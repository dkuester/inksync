/*
 * NickelInkSync plugin: pull the reading position from a KOSync server when a
 * sideloaded book is opened, push it when the book is left (firmware 5.18+,
 * Qt 6.5.2; Tolino Shine Color 5.18.264769).
 *
 * The plugin has no sync logic of its own. It calls the nickelinksync CLI in
 * /mnt/onboard/.adds/NickelInkSync/ with the configuration next to it, which
 * reads the device database (read-only while Nickel runs, `--live`), talks to
 * the server and applies the conflict rule.
 *
 * Opening (ReadingController::setVolume(Volume const&, Bookmark const&)):
 *   The hook asks the CLI (`pull --live --emit`) and, if there is a position
 *   to take, sets it (Content::setChapterIdBookmarked, setPercentRead) in
 *   the copy of the Volume handed to setVolume and in the Volume that
 *   ReadingView::setupReader loads again through VolumeManager::getById
 *   (only that book, only while the original setVolume runs). On the
 *   Tolino that opens the right chapter, but at its start: Nickel drops the
 *   place in the chapter (device tests, docs/nickel-5.18-symbols.md). So
 *   once the book is open, the plugin jumps there with Nickel's own
 *   navigation: it takes Nickel's bookmark for the current location, sets
 *   the pulled position as its start (ContentLocation::create,
 *   Bookmark::setStartLocation) and calls ReadingView::goToBookmark.
 *   Nothing is written to the database; Nickel saves the position itself.
 *   Only for sideloaded books opened without a bookmark (not a link or a
 *   search result), and with a time limit so a missing network never
 *   blocks for long.
 *
 * Leaving (ReadingView::leavingReadingExperience()):
 *   The original saves the position (saveProgress); after it returns, the CLI
 *   pushes that book in the background (`push --live`), so leaving the book
 *   never waits for the network.
 *
 * Network: Nickel switches Wi-Fi off when idle. Before pulling, the hook
 * switches it on (WirelessWorkflowManager::connectWireless, as NickelStorygraph
 * does) and waits in a local event loop, so the UI keeps running; before
 * pushing it does the same without waiting, polling once a second.
 *
 * Diagnosis (device tests of the open path): every getById of the opened
 * book with its caller and whether it was changed, and where Nickel stands
 * 3 s and 10 s after opening (ReadingView::bookmarkForCurrentLocation).
 *
 * Symbols and signatures: docs/nickel-5.18-symbols.md.
 *
 * Tolino only for now: init() leaves the plugin inactive on a Kobo (the
 * position formats differ, see SCOPE.md), and without the CLI or config.ini.
 * Log: NickelInkSync.log next to the CLI.
 */

#include <NickelHook.h>

#include <dlfcn.h>

#include <QByteArray>
#include <QCoreApplication>
#include <QDateTime>
#include <QEventLoop>
#include <QFile>
#include <QFileInfo>
#include <QProcess>
#include <QString>
#include <QStringList>
#include <QTimer>
#include <QVariant>

typedef void Device;
typedef void Volume;
typedef void Bookmark;
typedef void ReadingController;
typedef void ReadingView;
typedef void WirelessWorkflowManager;
typedef void MainWindowController;
typedef void ContentLocation;

static const char kDir[]    = "/mnt/onboard/.adds/NickelInkSync";
static const char kCli[]    = "/mnt/onboard/.adds/NickelInkSync/nickelinksync";
static const char kConfig[] = "/mnt/onboard/.adds/NickelInkSync/config.ini";
static const char kLog[]    = "/mnt/onboard/.adds/NickelInkSync/NickelInkSync.log";
static const char kOnboard[] = "/mnt/onboard";
static const char kSideloaded[] = "file:///mnt/onboard/";
static const qint64 kLogMaxBytes = 128 * 1024;
// Seconds per request for pull, and how long opening the book may wait in
// total (CLI start, database, hash, two requests).
static const char kPullTimeout[] = "6";
static const int kPullWaitMs = 15000;
// A Volume is 12 bytes in 5.18 (vtable, shared data, flags; see the copy
// constructor); room to spare.
static const size_t kVolumeBytes = 64;
// A Bookmark is 8 bytes (vtable, shared data; see its constructor).
static const size_t kBookmarkBytes = 64;
// How long opening a book may wait for Wi-Fi, and leaving it.
static const int kWifiOpenWaitMs = 12000;
static const int kWifiLeaveWaitS = 45;
// How long getById calls of the opened book are logged.
static const int kWatchMs = 20000;
// A ContentLocation is 8 bytes (a shared_ptr; see its destructor).
static const size_t kLocationBytes = 64;
// How often and how long the jump waits for the book to be open.
static const int kJumpPollMs = 300;
static const int kJumpTries = 50;

// Originals, filled by NickelHook (.out).
static void (*ReadingController__setVolume)(ReadingController *, const Volume *, const Bookmark *);
static void (*ReadingView__leavingReadingExperience)(ReadingView *);
// static Volume VolumeManager::getById(QString const& id, QString const&):
// the Volume is returned through a hidden pointer (r0), which it also
// returns in r0; the arguments follow in r1, r2.
static Volume *(*VolumeManager__getById)(Volume *, const QString *, const QString *);

// Resolved via dlsym.
static Device *(*Device__getCurrentDevice)();  // returns Device& (r0)
static bool (*Device__isTolino)(const Device *);
// QString is returned through a hidden pointer (r0), `this` follows in r1;
// declaring the return type as QString gives exactly that ABI. The argument
// is unused by 5.18.
static QString (*Content__getId)(const Volume *, const QString &);
static bool (*Bookmark__isNull)(const Bookmark *);
static void (*Volume__copy)(Volume *, const Volume *);  // Volume(Volume const&)
static void (*Volume__dtor)(Volume *);
static void (*Content__setChapterIdBookmarked)(Volume *, const QVariant &);
static void (*Content__setPercentRead)(Volume *, const QVariant &);

// Optional (NULL if missing): network and diagnosis.
static WirelessWorkflowManager *(*WirelessWorkflowManager__sharedInstance)();
static bool (*WirelessWorkflowManager__isInternetAccessible)(WirelessWorkflowManager *);
static void (*WirelessWorkflowManager__connectWireless)(WirelessWorkflowManager *, bool, bool);
static MainWindowController *(*MainWindowController__sharedInstance)();
// Returns a QWidget*; QObject is its first base, so the pointer is the same.
static QObject *(*MainWindowController__currentView)(const MainWindowController *);
// Bookmark by value: hidden result pointer in r0, `this` in r1.
static void (*ReadingView__bookmarkForCurrentLocation)(void *result, ReadingView *);
// Returns a reference (d + 0x104), not a value.
static const ContentLocation *(*Bookmark__getStartLocation)(const Bookmark *);
static QString (*ContentLocation__toString)(const ContentLocation *);
static void (*Bookmark__dtor)(Bookmark *);

// Optional: the jump after opening.
static bool (*ReadingView__isBookOpened)(const ReadingView *);
static bool (*ReadingView__goToBookmark)(ReadingView *, const Bookmark *);
static void (*Bookmark__setStartLocation)(Bookmark *, const ContentLocation *);
// static ContentLocation create(QString position, QString spinePath):
// result through a hidden pointer (r0); the QStrings by value arrive as
// pointers to the caller's copies. A "#point(" position is the RMSDK
// format, which needs no spinePath.
static void (*ContentLocation__create)(void *result, QString *, QString *);
static bool (*ContentLocation__isValid)(const ContentLocation *);
static void (*ContentLocation__dtor)(ContentLocation *);

// Set at the end of a successful init(); every hook passes through until then.
static bool g_active = false;
// ContentID of the book opened last (GUI thread only).
static QString g_open_id;

// The position to set while the original setVolume runs (GUI thread only):
// VolumeManager::getById applies it to the Volume of `id` it loads.
struct Pending {
    bool active = false;
    bool applied = false;
    QString id;
    QByteArray position;
    int percent = 0;
};
static Pending g_pending;

// getById calls of the opened book are logged until `until` (diagnosis).
struct Watch {
    QString id;
    QDateTime until;
    int calls = 0;
};
static Watch g_watch;

static void log_line(const QString &msg)
{
    nh_log("%s", qPrintable(msg));
    QFile f(QString::fromLatin1(kLog));
    if (f.exists() && f.size() > kLogMaxBytes) {
        f.remove();
    }
    if (f.open(QIODevice::WriteOnly | QIODevice::Append | QIODevice::Text)) {
        f.write(QDateTime::currentDateTime().toString(Qt::ISODate).toUtf8());
        f.write(" ");
        f.write(msg.toUtf8());
        f.write("\n");
    }
}

static bool online()
{
    return WirelessWorkflowManager__sharedInstance && WirelessWorkflowManager__isInternetAccessible
        && WirelessWorkflowManager__isInternetAccessible(WirelessWorkflowManager__sharedInstance());
}

// Asks Nickel to connect, from the event loop (NickelStorygraph does the
// same; connectWirelessSilently can stall the event loop).
static bool connect_wifi()
{
    if (!WirelessWorkflowManager__sharedInstance || !WirelessWorkflowManager__connectWireless) {
        return false;
    }
    QTimer::singleShot(0, qApp, [] {
        WirelessWorkflowManager__connectWireless(WirelessWorkflowManager__sharedInstance(), false, false);
    });
    return true;
}

// Waits until the network is there, at most `ms`, in a local event loop so
// the UI keeps running.
static bool wait_online(int ms)
{
    if (online()) {
        return true;
    }
    if (!connect_wifi()) {
        return false;
    }
    log_line(QStringLiteral("no network: switching Wi-Fi on, waiting up to %1 s").arg(ms / 1000));
    QEventLoop loop;
    QTimer poll;
    QObject::connect(&poll, &QTimer::timeout, &loop, [&loop] {
        if (online()) {
            loop.quit();
        }
    });
    QTimer::singleShot(ms, &loop, &QEventLoop::quit);
    poll.start(500);
    loop.exec();
    const bool ok = online();
    log_line(ok ? QStringLiteral("network is there") : QStringLiteral("still no network"));
    return ok;
}

// Runs `fn` once the network is there, polling once a second for at most
// `seconds`, without waiting.
template <typename F>
static void when_online(int seconds, F fn)
{
    if (online() || !connect_wifi()) {
        fn();
        return;
    }
    log_line(QStringLiteral("no network: switching Wi-Fi on, waiting up to %1 s in the background").arg(seconds));
    QTimer *t = new QTimer(qApp);
    QObject::connect(t, &QTimer::timeout, t, [t, fn, seconds, n = 0]() mutable {
        if (online()) {
            t->deleteLater();
            fn();
        } else if (++n >= seconds) {
            t->deleteLater();
            log_line(QStringLiteral("still no network after %1 s: nothing sent").arg(seconds));
        }
    });
    t->start(1000);
}

// The reading view, if it is the current view.
static ReadingView *reading_view()
{
    if (!MainWindowController__sharedInstance || !MainWindowController__currentView) {
        return nullptr;
    }
    QObject *view = MainWindowController__currentView(MainWindowController__sharedInstance());
    return view && view->objectName() == QLatin1String("ReadingView") ? view : nullptr;
}

// Where Nickel stands in the open book (diagnosis).
static void log_location(const QString &when)
{
    if (!ReadingView__bookmarkForCurrentLocation || !Bookmark__getStartLocation
        || !ContentLocation__toString || !Bookmark__dtor) {
        return;
    }
    ReadingView *view = reading_view();
    if (!view) {
        log_line(QStringLiteral("%1: not in the reading view").arg(when));
        return;
    }
    alignas(8) unsigned char bookmark[kBookmarkBytes];
    ReadingView__bookmarkForCurrentLocation(bookmark, view);
    const QString at = ContentLocation__toString(Bookmark__getStartLocation(bookmark));
    Bookmark__dtor(bookmark);
    log_line(QStringLiteral("%1: Nickel is at %2").arg(when, at));
}

// Moves the open book `id` to `position` once Nickel has opened it, through
// Nickel's own navigation; tries every kJumpPollMs, kJumpTries times.
static void jump_when_open(const QString &id, const QByteArray &position, int tries = 0)
{
    if (!ReadingView__isBookOpened || !ReadingView__goToBookmark || !Bookmark__setStartLocation
        || !ContentLocation__create || !ContentLocation__isValid || !ContentLocation__dtor
        || !ReadingView__bookmarkForCurrentLocation || !Bookmark__getStartLocation
        || !ContentLocation__toString || !Bookmark__dtor) {
        log_line(QStringLiteral("jump: symbols missing, not jumping"));
        return;
    }
    if (g_open_id != id) {
        return;  // another book was opened meanwhile
    }
    ReadingView *view = reading_view();
    if (!view || !ReadingView__isBookOpened(view)) {
        if (tries + 1 >= kJumpTries) {
            log_line(QStringLiteral("jump: the book did not open within %1 s").arg(kJumpTries * kJumpPollMs / 1000));
            return;
        }
        QTimer::singleShot(kJumpPollMs, qApp, [id, position, tries] { jump_when_open(id, position, tries + 1); });
        return;
    }
    const QString target = QString::fromUtf8(position);
    alignas(8) unsigned char bookmark[kBookmarkBytes];
    ReadingView__bookmarkForCurrentLocation(bookmark, view);
    const QString at = ContentLocation__toString(Bookmark__getStartLocation(bookmark));
    if (at == target) {
        log_line(QStringLiteral("jump: Nickel is already at %1").arg(at));
        Bookmark__dtor(bookmark);
        return;
    }
    QString location_arg = target;
    QString spine_path;
    alignas(8) unsigned char location[kLocationBytes];
    ContentLocation__create(location, &location_arg, &spine_path);
    if (!ContentLocation__isValid(location)) {
        log_line(QStringLiteral("jump: Nickel does not take %1 as a location").arg(target));
    } else {
        Bookmark__setStartLocation(bookmark, location);
        const bool ok = ReadingView__goToBookmark(view, bookmark);
        log_line(QStringLiteral("jump from %1 to %2 after %3 ms: %4")
                     .arg(at, target)
                     .arg((tries + 1) * kJumpPollMs)
                     .arg(ok ? QStringLiteral("done") : QStringLiteral("refused")));
        QTimer::singleShot(2000, qApp, [] { log_location(QStringLiteral("2 s after the jump")); });
    }
    ContentLocation__dtor(location);
    Bookmark__dtor(bookmark);
}

// Logs where Nickel stands 3 s and 10 s after the book was opened.
static void log_locations_later()
{
    QTimer::singleShot(3000, qApp, [] { log_location(QStringLiteral("3 s after opening")); });
    QTimer::singleShot(10000, qApp, [] { log_location(QStringLiteral("10 s after opening")); });
}

// The exported libnickel function a return address lies in (diagnosis).
static QString caller(void *address)
{
    Dl_info info;
    if (dladdr(address, &info) && info.dli_sname) {
        return QString::fromLatin1(info.dli_sname);
    }
    return QStringLiteral("(unknown)");
}

// The common arguments for one book: device, configuration, book, live.
static QStringList book_args(const char *command, const QString &id)
{
    return QStringList{
        QString::fromLatin1(command),
        QStringLiteral("--live"),
        QStringLiteral("--device"), QString::fromLatin1(kOnboard),
        QStringLiteral("--config"), QString::fromLatin1(kConfig),
        QStringLiteral("--content-id"), id,
    };
}

struct Pulled {
    bool found = false;
    int percent = 0;
    QByteArray position;
};

// Runs `pull --live --emit` for one book and reads the position to take
// from its `NICKELINKSYNC_POSITION<tab>percent<tab>position` line.
static Pulled pull(const QString &id)
{
    Pulled result;
    QStringList args = book_args("pull", id);
    args << QStringLiteral("--emit") << QStringLiteral("--timeout") << QString::fromLatin1(kPullTimeout);
    QProcess p;
    p.setProcessChannelMode(QProcess::MergedChannels);
    p.start(QString::fromLatin1(kCli), args);
    if (!p.waitForStarted(3000)) {
        log_line(QStringLiteral("pull: could not start the CLI: %1").arg(p.errorString()));
        return result;
    }
    if (!p.waitForFinished(kPullWaitMs)) {
        p.kill();
        p.waitForFinished(1000);
        log_line(QStringLiteral("pull: no answer within %1 s, book opens unchanged").arg(kPullWaitMs / 1000));
        return result;
    }
    const QByteArray out = p.readAll();
    log_line(QStringLiteral("pull %1 (exit %2):\n%3").arg(id).arg(p.exitCode()).arg(QString::fromUtf8(out).trimmed()));
    for (const QByteArray &line : out.split('\n')) {
        const QList<QByteArray> f = line.trimmed().split('\t');
        if (f.size() == 3 && f[0] == "NICKELINKSYNC_POSITION" && !f[2].isEmpty()) {
            bool ok = false;
            const int percent = f[1].toInt(&ok);
            if (ok) {
                result.found = true;
                result.percent = percent;
                result.position = f[2];
            }
        }
    }
    return result;
}

extern "C" __attribute__((visibility("default")))
void _nh_ReadingController__setVolume(ReadingController *self, const Volume *volume, const Bookmark *bookmark)
{
    if (!g_active) {
        ReadingController__setVolume(self, volume, bookmark);
        return;
    }
    const QString id = Content__getId(volume, QString());
    g_open_id = id;
    if (!id.startsWith(QString::fromLatin1(kSideloaded))) {
        ReadingController__setVolume(self, volume, bookmark);
        return;
    }
    if (!Bookmark__isNull(bookmark)) {
        log_line(QStringLiteral("open %1 with a bookmark (link, search): not pulled").arg(id));
        ReadingController__setVolume(self, volume, bookmark);
        return;
    }
    if (!wait_online(kWifiOpenWaitMs)) {
        log_line(QStringLiteral("open %1 unchanged: no network").arg(id));
        ReadingController__setVolume(self, volume, bookmark);
        return;
    }
    const Pulled p = pull(id);
    g_watch.id = id;
    g_watch.until = QDateTime::currentDateTimeUtc().addMSecs(kWatchMs);
    g_watch.calls = 0;
    if (!p.found) {
        ReadingController__setVolume(self, volume, bookmark);
        log_locations_later();
        return;
    }
    log_line(QStringLiteral("open %1 at %2 (%3 %)").arg(id, QString::fromUtf8(p.position)).arg(p.percent));
    alignas(8) unsigned char copy[kVolumeBytes];
    Volume__copy(copy, volume);
    Content__setChapterIdBookmarked(copy, QVariant(p.position));
    Content__setPercentRead(copy, QVariant(p.percent));
    g_pending.active = true;
    g_pending.applied = false;
    g_pending.id = id;
    g_pending.position = p.position;
    g_pending.percent = p.percent;
    ReadingController__setVolume(self, copy, bookmark);
    if (!g_pending.applied) {
        log_line(QStringLiteral("open %1: Nickel did not load the book again while opening").arg(id));
    }
    g_pending = Pending();
    Volume__dtor(copy);
    log_locations_later();
    jump_when_open(id, p.position);
}

extern "C" __attribute__((visibility("default")))
Volume *_nh_VolumeManager__getById(Volume *out, const QString *id, const QString *other)
{
    VolumeManager__getById(out, id, other);
    const bool patch = g_pending.active && *id == g_pending.id;
    if (patch) {
        Content__setChapterIdBookmarked(out, QVariant(g_pending.position));
        Content__setPercentRead(out, QVariant(g_pending.percent));
        g_pending.applied = true;
    }
    if (*id == g_watch.id && QDateTime::currentDateTimeUtc() < g_watch.until) {
        log_line(QStringLiteral("getById #%1 by %2, %3, %4")
                     .arg(++g_watch.calls)
                     .arg(caller(__builtin_return_address(0)),
                          g_pending.active ? QStringLiteral("while opening") : QStringLiteral("after opening"),
                          patch ? QStringLiteral("position set") : QStringLiteral("unchanged")));
    }
    return out;
}

extern "C" __attribute__((visibility("default")))
void _nh_ReadingView__leavingReadingExperience(ReadingView *self)
{
    // First Nickel saves the position.
    ReadingView__leavingReadingExperience(self);
    if (!g_active || !g_open_id.startsWith(QString::fromLatin1(kSideloaded))) {
        return;
    }
    // Detached, output appended to the log; the arguments reach the shell
    // as $0.. so nothing needs quoting.
    QStringList args{
        QStringLiteral("-c"),
        QStringLiteral("exec \"$0\" \"$@\" >>\"%1\" 2>&1").arg(QString::fromLatin1(kLog)),
        QString::fromLatin1(kCli),
    };
    args << book_args("push", g_open_id);
    const QString id = g_open_id;
    when_online(kWifiLeaveWaitS, [args, id] {
        log_line(QStringLiteral("push %1").arg(id));
        if (!QProcess::startDetached(QStringLiteral("/bin/sh"), args)) {
            log_line(QStringLiteral("push: could not start the CLI"));
        }
    });
}

static int nickelinksync_init()
{
    if (!Device__isTolino(Device__getCurrentDevice())) {
        log_line(QStringLiteral("inactive: not a Tolino (the Kobo follows later)"));
        return 0;
    }
    if (!QFileInfo(QString::fromLatin1(kCli)).isExecutable()) {
        log_line(QStringLiteral("inactive: %1 is missing or not executable").arg(QString::fromLatin1(kCli)));
        return 0;
    }
    if (!QFileInfo::exists(QString::fromLatin1(kConfig))) {
        log_line(QStringLiteral("inactive: %1 is missing").arg(QString::fromLatin1(kConfig)));
        return 0;
    }
    log_line(QStringLiteral("active, CLI and configuration in %1").arg(QString::fromLatin1(kDir)));
    g_active = true;
    return 0;
}

static struct nh_info NickelInkSync = {
    .name = "NickelInkSync",
    .desc = "Sync the reading position of sideloaded books with a KOSync server (Tolino 5.18+)",
    .uninstall_flag = "/mnt/onboard/.adds/NickelInkSync/uninstall",
};

static struct nh_hook NickelInkSyncHook[] = {
    {
        .sym     = "_ZN17ReadingController9setVolumeERK6VolumeRK8Bookmark",
        .sym_new = "_nh_ReadingController__setVolume",
        .lib     = "libnickel.so.1.0.0",
        .out     = nh_symoutptr(ReadingController__setVolume),
        .desc    = "Pull the position when a book is opened",
    },
    {
        .sym     = "_ZN11ReadingView24leavingReadingExperienceEv",
        .sym_new = "_nh_ReadingView__leavingReadingExperience",
        .lib     = "libnickel.so.1.0.0",
        .out     = nh_symoutptr(ReadingView__leavingReadingExperience),
        .desc    = "Push the position when a book is left",
    },
    {
        .sym     = "_ZN13VolumeManager7getByIdERK7QStringS2_",
        .sym_new = "_nh_VolumeManager__getById",
        .lib     = "libnickel.so.1.0.0",
        .out     = nh_symoutptr(VolumeManager__getById),
        .desc    = "Set the pulled position in the book Nickel loads while opening it",
    },
    {0},
};

static struct nh_dlsym NickelInkSyncDlsym[] = {
    {.name = "_ZN6Device16getCurrentDeviceEv", .out = nh_symoutptr(Device__getCurrentDevice), .desc = "Tolino check"},
    {.name = "_ZNK6Device8isTolinoEv", .out = nh_symoutptr(Device__isTolino), .desc = "Tolino check"},
    {.name = "_ZNK7Content5getIdERK7QString", .out = nh_symoutptr(Content__getId), .desc = "Book ID"},
    {.name = "_ZNK8Bookmark6isNullEv", .out = nh_symoutptr(Bookmark__isNull), .desc = "Opened without a bookmark"},
    {.name = "_ZN6VolumeC1ERKS_", .out = nh_symoutptr(Volume__copy), .desc = "Copy of the book to open"},
    {.name = "_ZN6VolumeD1Ev", .out = nh_symoutptr(Volume__dtor), .desc = "Copy of the book to open"},
    {.name = "_ZN7Content22setChapterIdBookmarkedERK8QVariant", .out = nh_symoutptr(Content__setChapterIdBookmarked), .desc = "Pulled position"},
    {.name = "_ZN7Content14setPercentReadERK8QVariant", .out = nh_symoutptr(Content__setPercentRead), .desc = "Pulled percentage"},
    {.name = "_ZN23WirelessWorkflowManager14sharedInstanceEv", .out = nh_symoutptr(WirelessWorkflowManager__sharedInstance), .desc = "Wi-Fi", .optional = true},
    {.name = "_ZN23WirelessWorkflowManager20isInternetAccessibleEv", .out = nh_symoutptr(WirelessWorkflowManager__isInternetAccessible), .desc = "Wi-Fi", .optional = true},
    {.name = "_ZN23WirelessWorkflowManager15connectWirelessEbb", .out = nh_symoutptr(WirelessWorkflowManager__connectWireless), .desc = "Wi-Fi", .optional = true},
    {.name = "_ZN20MainWindowController14sharedInstanceEv", .out = nh_symoutptr(MainWindowController__sharedInstance), .desc = "Diagnosis", .optional = true},
    {.name = "_ZNK20MainWindowController11currentViewEv", .out = nh_symoutptr(MainWindowController__currentView), .desc = "Diagnosis", .optional = true},
    {.name = "_ZN11ReadingView26bookmarkForCurrentLocationEv", .out = nh_symoutptr(ReadingView__bookmarkForCurrentLocation), .desc = "Diagnosis", .optional = true},
    {.name = "_ZNK8Bookmark16getStartLocationEv", .out = nh_symoutptr(Bookmark__getStartLocation), .desc = "Diagnosis", .optional = true},
    {.name = "_ZNK15ContentLocation8toStringEv", .out = nh_symoutptr(ContentLocation__toString), .desc = "Diagnosis", .optional = true},
    {.name = "_ZN8BookmarkD1Ev", .out = nh_symoutptr(Bookmark__dtor), .desc = "Diagnosis, jump", .optional = true},
    {.name = "_ZNK11ReadingView12isBookOpenedEv", .out = nh_symoutptr(ReadingView__isBookOpened), .desc = "Jump", .optional = true},
    {.name = "_ZN11ReadingView12goToBookmarkERK8Bookmark", .out = nh_symoutptr(ReadingView__goToBookmark), .desc = "Jump", .optional = true},
    {.name = "_ZN8Bookmark16setStartLocationERK15ContentLocation", .out = nh_symoutptr(Bookmark__setStartLocation), .desc = "Jump", .optional = true},
    {.name = "_ZN15ContentLocation6createE7QStringS0_", .out = nh_symoutptr(ContentLocation__create), .desc = "Jump", .optional = true},
    {.name = "_ZNK15ContentLocation7isValidEv", .out = nh_symoutptr(ContentLocation__isValid), .desc = "Jump", .optional = true},
    {.name = "_ZN15ContentLocationD1Ev", .out = nh_symoutptr(ContentLocation__dtor), .desc = "Jump", .optional = true},
    {0},
};

NickelHook(
    .init  = nickelinksync_init,
    .info  = &NickelInkSync,
    .hook  = NickelInkSyncHook,
    .dlsym = NickelInkSyncDlsym,
)
