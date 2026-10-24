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
 *   The book opens at once, at the device's position. In the background the
 *   plugin switches Wi-Fi on, asks the CLI (`pull --live --emit`) and, if
 *   there is a position to take, jumps there with Nickel's own navigation:
 *   it takes Nickel's bookmark for the current location, sets the pulled
 *   position as its start (ContentLocation::create,
 *   Bookmark::setStartLocation) and calls ReadingView::goToBookmark. Nothing
 *   is written to the database; Nickel saves the position itself. Only for
 *   sideloaded books opened without a bookmark (not a link or a search
 *   result).
 *
 *   Device tests showed why not before opening: setting the position in the
 *   Volume Nickel opens gets the chapter across but never the place in it,
 *   and waiting for Wi-Fi before opening made opening slow
 *   (docs/nickel-5.18-symbols.md).
 *
 * Leaving (ReadingView::leavingReadingExperience()):
 *   The original saves the position (saveProgress); after it returns, the CLI
 *   pushes that book in the background (`push --live`), once Wi-Fi is up. Not
 *   while the background pull of that book still runs: then the reading view
 *   was only covered (a dialog), and its position is about to change. And
 *   not after a jump that did not reach the pulled chapter: the device
 *   position would then overwrite the newer one on the server (device test).
 *
 * Network: Nickel switches Wi-Fi off when idle. The plugin asks it to connect
 * without a dialog first (WirelessWorkflowManager::connectWireless(false,
 * true), which is what connectWirelessSilently() calls) and polls once a
 * second; if that brings no network within kSilentWifiS, it asks again with
 * Nickel's own Wi-Fi dialog (connectWireless(false, false)), where the
 * reader can pick a network.
 *
 * Symbols and signatures: docs/nickel-5.18-symbols.md.
 *
 * Tolino only for now: init() leaves the plugin inactive on a Kobo (the
 * position formats differ, see SCOPE.md), and without the CLI or config.ini.
 * Log: NickelInkSync.log next to the CLI.
 */

#include <NickelHook.h>

#include <QByteArray>
#include <QCoreApplication>
#include <QDateTime>
#include <QFile>
#include <QFileInfo>
#include <QProcess>
#include <QString>
#include <QStringList>
#include <QTimer>

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
// Seconds per request for pull, and how long the pull may take as a whole
// (CLI start, database, hash, two requests).
static const char kPullTimeout[] = "8";
static const int kPullWaitMs = 25000;
// How long to wait for Wi-Fi after opening and after leaving a book, in
// all; the first kSilentWifiS without a dialog.
static const int kWifiOpenWaitS = 60;
static const int kWifiLeaveWaitS = 60;
static const int kSilentWifiS = 15;
// A Bookmark is 8 bytes (vtable, shared data; see its constructor), a
// ContentLocation too (a shared_ptr; see its destructor). Room to spare.
static const size_t kBookmarkBytes = 64;
static const size_t kLocationBytes = 64;
// How often and how long the jump waits for the reading view.
static const int kJumpPollMs = 300;
static const int kJumpTries = 50;

// Originals, filled by NickelHook (.out).
static void (*ReadingController__setVolume)(ReadingController *, const Volume *, const Bookmark *);
static void (*ReadingView__leavingReadingExperience)(ReadingView *);

// Resolved via dlsym.
static Device *(*Device__getCurrentDevice)();  // returns Device& (r0)
static bool (*Device__isTolino)(const Device *);
// QString is returned through a hidden pointer (r0), `this` follows in r1;
// declaring the return type as QString gives exactly that ABI. The argument
// is unused by 5.18.
static QString (*Content__getId)(const Volume *, const QString &);
static bool (*Bookmark__isNull)(const Bookmark *);
static MainWindowController *(*MainWindowController__sharedInstance)();
// Returns a QWidget*; QObject is its first base, so the pointer is the same.
static QObject *(*MainWindowController__currentView)(const MainWindowController *);
// Bookmark by value: hidden result pointer in r0, `this` in r1.
static void (*ReadingView__bookmarkForCurrentLocation)(void *result, ReadingView *);
// Returns a reference (d + 0x104), not a value.
static const ContentLocation *(*Bookmark__getStartLocation)(const Bookmark *);
static QString (*ContentLocation__toString)(const ContentLocation *);
static void (*Bookmark__dtor)(Bookmark *);
static bool (*ReadingView__goToBookmark)(ReadingView *, const Bookmark *);
static void (*Bookmark__setStartLocation)(Bookmark *, const ContentLocation *);
// static ContentLocation create(QString position, QString spinePath):
// result through a hidden pointer (r0); the QStrings by value arrive as
// pointers to the caller's copies. A "#point(" position is the RMSDK
// format, which needs no spinePath.
static void (*ContentLocation__create)(void *result, QString *, QString *);
static bool (*ContentLocation__isValid)(const ContentLocation *);
static void (*ContentLocation__dtor)(ContentLocation *);

// Optional (NULL if missing): without them the plugin tries without Wi-Fi.
static WirelessWorkflowManager *(*WirelessWorkflowManager__sharedInstance)();
static bool (*WirelessWorkflowManager__isInternetAccessible)(WirelessWorkflowManager *);
static void (*WirelessWorkflowManager__connectWireless)(WirelessWorkflowManager *, bool, bool);

// Set at the end of a successful init(); every hook passes through until then.
static bool g_active = false;
// ContentID of the book opened last (GUI thread only).
static QString g_open_id;
// The book whose background pull and jump still run (GUI thread only).
static QString g_syncing;
// The book whose pulled position Nickel did not reach: not pushed.
static QString g_not_reached;

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

static void sync_done(const QString &id)
{
    if (g_syncing == id) {
        g_syncing.clear();
    }
}

static bool online()
{
    return WirelessWorkflowManager__sharedInstance && WirelessWorkflowManager__isInternetAccessible
        && WirelessWorkflowManager__isInternetAccessible(WirelessWorkflowManager__sharedInstance());
}

// Asks Nickel to connect, from the event loop: `silent` to a known network
// without a dialog, otherwise with Nickel's Wi-Fi dialog, which shows the
// network list when it cannot connect at once (device test).
static bool connect_wifi(bool silent)
{
    if (!WirelessWorkflowManager__sharedInstance || !WirelessWorkflowManager__connectWireless) {
        return false;
    }
    QTimer::singleShot(0, qApp, [silent] {
        WirelessWorkflowManager__connectWireless(WirelessWorkflowManager__sharedInstance(), false, silent);
    });
    return true;
}

// Runs `fn` once the network is there, polling once a second for at most
// `seconds`, without waiting; `failed` if it never comes. Silently first,
// after kSilentWifiS with Nickel's Wi-Fi dialog. Without the Wi-Fi symbols
// it just tries.
template <typename F, typename G>
static void when_online(int seconds, F fn, G failed)
{
    if (online() || !connect_wifi(true)) {
        fn();
        return;
    }
    log_line(QStringLiteral("no network: switching Wi-Fi on, waiting up to %1 s").arg(seconds));
    QTimer *t = new QTimer(qApp);
    QObject::connect(t, &QTimer::timeout, t, [t, fn, failed, seconds, n = 0]() mutable {
        if (online()) {
            t->deleteLater();
            log_line(QStringLiteral("network is there after %1 s").arg(n + 1));
            fn();
        } else if (++n == kSilentWifiS) {
            log_line(QStringLiteral("no network after %1 s: showing Nickel's Wi-Fi dialog").arg(n));
            connect_wifi(false);
        } else if (n >= seconds) {
            t->deleteLater();
            log_line(QStringLiteral("still no network after %1 s").arg(seconds));
            failed();
        }
    });
    t->start(1000);
}

// The reading view, if it is the current view.
static ReadingView *reading_view()
{
    QObject *view = MainWindowController__currentView(MainWindowController__sharedInstance());
    return view && view->objectName() == QLatin1String("ReadingView") ? view : nullptr;
}

// Where Nickel stands in the open book, or an empty string.
static QString current_location(ReadingView *view)
{
    alignas(8) unsigned char bookmark[kBookmarkBytes];
    ReadingView__bookmarkForCurrentLocation(bookmark, view);
    const QString at = ContentLocation__toString(Bookmark__getStartLocation(bookmark));
    Bookmark__dtor(bookmark);
    return at;
}

// The chapter file of a position (`OEBPS/text/chapter1.xhtml#point(...)`).
static QString chapter_of(const QString &position)
{
    return position.section(QLatin1Char('#'), 0, 0);
}

// After a jump: Nickel shows a page start, which lies before the pulled
// point, so "reached" means the same chapter.
static void check_jump(const QString &id, const QString &target)
{
    ReadingView *view = reading_view();
    if (!view || g_open_id != id) {
        log_line(QStringLiteral("2 s after the jump: not in the reading view of the book"));
        return;
    }
    const QString at = current_location(view);
    if (chapter_of(at) == chapter_of(target)) {
        if (g_not_reached == id) {
            g_not_reached.clear();
        }
        log_line(QStringLiteral("2 s after the jump: Nickel is at %1, in the pulled chapter").arg(at));
    } else {
        log_line(QStringLiteral("2 s after the jump: Nickel is still at %1, not in the pulled chapter; "
                                "leaving this book will not push").arg(at));
    }
}

// Moves the open book `id` to `position` through Nickel's own navigation,
// once the reading view is current and reports the same location twice in
// a row (ReadingView::isBookOpened() stays false for a plain EPUB on the
// Tolino). Tries every kJumpPollMs, kJumpTries times.
static void jump_when_open(const QString &id, const QByteArray &position, int tries = 0, const QString &last = QString())
{
    if (g_open_id != id) {
        log_line(QStringLiteral("jump: another book is open, not jumping"));
        sync_done(id);
        return;
    }
    ReadingView *view = reading_view();
    const QString at = view ? current_location(view) : QString();
    if (at.isEmpty() || at != last) {
        if (tries + 1 >= kJumpTries) {
            log_line(QStringLiteral("jump: the reading view did not settle within %1 s").arg(kJumpTries * kJumpPollMs / 1000));
            sync_done(id);
            return;
        }
        QTimer::singleShot(kJumpPollMs, qApp, [id, position, tries, at] { jump_when_open(id, position, tries + 1, at); });
        return;
    }
    const QString target = QString::fromUtf8(position);
    if (at == target) {
        log_line(QStringLiteral("jump: Nickel is already at %1").arg(at));
        sync_done(id);
        return;
    }
    QString location_arg = target;
    QString spine_path;
    alignas(8) unsigned char location[kLocationBytes];
    ContentLocation__create(location, &location_arg, &spine_path);
    if (!ContentLocation__isValid(location)) {
        log_line(QStringLiteral("jump: Nickel does not take %1 as a location").arg(target));
    } else {
        alignas(8) unsigned char bookmark[kBookmarkBytes];
        ReadingView__bookmarkForCurrentLocation(bookmark, view);
        Bookmark__setStartLocation(bookmark, location);
        const bool ok = ReadingView__goToBookmark(view, bookmark);
        Bookmark__dtor(bookmark);
        log_line(QStringLiteral("jump from %1 to %2: %3")
                     .arg(at, target, ok ? QStringLiteral("done") : QStringLiteral("refused")));
        // Until checked, the pulled position counts as not reached.
        g_not_reached = id;
        QTimer::singleShot(2000, qApp, [id, target] { check_jump(id, target); });
    }
    ContentLocation__dtor(location);
    sync_done(id);
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

// Runs `pull --live --emit` for one book without waiting and jumps to the
// position from its `NICKELINKSYNC_POSITION<tab>percent<tab>position` line.
static void pull_and_jump(const QString &id)
{
    QStringList args = book_args("pull", id);
    args << QStringLiteral("--emit") << QStringLiteral("--timeout") << QString::fromLatin1(kPullTimeout);
    QProcess *p = new QProcess(qApp);
    p->setProcessChannelMode(QProcess::MergedChannels);
    QObject::connect(p, &QProcess::errorOccurred, p, [p, id](QProcess::ProcessError e) {
        if (e == QProcess::FailedToStart) {
            log_line(QStringLiteral("pull: could not start the CLI: %1").arg(p->errorString()));
            p->deleteLater();
            sync_done(id);
        }
    });
    QObject::connect(p, &QProcess::finished, p, [p, id](int code, QProcess::ExitStatus status) {
        const QByteArray out = p->readAll();
        p->deleteLater();
        log_line(QStringLiteral("pull %1 (%2):\n%3")
                     .arg(id,
                          status == QProcess::NormalExit ? QStringLiteral("exit %1").arg(code) : QStringLiteral("stopped"),
                          QString::fromUtf8(out).trimmed()));
        for (const QByteArray &line : out.split('\n')) {
            const QList<QByteArray> f = line.trimmed().split('\t');
            if (f.size() == 3 && f[0] == "NICKELINKSYNC_POSITION" && !f[2].isEmpty()) {
                jump_when_open(id, f[2]);
                return;
            }
        }
        sync_done(id);
    });
    // Never let a hanging request keep the book in "syncing".
    QTimer::singleShot(kPullWaitMs, p, [p] {
        if (p->state() != QProcess::NotRunning) {
            log_line(QStringLiteral("pull: no answer within %1 s, stopped").arg(kPullWaitMs / 1000));
            p->kill();
        }
    });
    p->start(QString::fromLatin1(kCli), args);
}

extern "C" __attribute__((visibility("default")))
void _nh_ReadingController__setVolume(ReadingController *self, const Volume *volume, const Bookmark *bookmark)
{
    // Open at once; the pulled position follows by a jump.
    ReadingController__setVolume(self, volume, bookmark);
    if (!g_active) {
        return;
    }
    const QString id = Content__getId(volume, QString());
    g_open_id = id;
    g_not_reached.clear();
    if (!id.startsWith(QString::fromLatin1(kSideloaded))) {
        return;
    }
    if (!Bookmark__isNull(bookmark)) {
        log_line(QStringLiteral("open %1 with a bookmark (link, search): not pulled").arg(id));
        return;
    }
    log_line(QStringLiteral("open %1: pulling in the background").arg(id));
    g_syncing = id;
    when_online(kWifiOpenWaitS, [id] { pull_and_jump(id); }, [id] { sync_done(id); });
}

extern "C" __attribute__((visibility("default")))
void _nh_ReadingView__leavingReadingExperience(ReadingView *self)
{
    // First Nickel saves the position.
    ReadingView__leavingReadingExperience(self);
    if (!g_active || !g_open_id.startsWith(QString::fromLatin1(kSideloaded))) {
        return;
    }
    const QString id = g_open_id;
    if (g_syncing == id) {
        log_line(QStringLiteral("left the reading view while pulling %1 (a dialog?): not pushed").arg(id));
        return;
    }
    if (g_not_reached == id) {
        log_line(QStringLiteral("left %1, but Nickel never reached the pulled position: not pushed").arg(id));
        return;
    }
    // Detached, output appended to the log; the arguments reach the shell
    // as $0.. so nothing needs quoting.
    QStringList args{
        QStringLiteral("-c"),
        QStringLiteral("exec \"$0\" \"$@\" >>\"%1\" 2>&1").arg(QString::fromLatin1(kLog)),
        QString::fromLatin1(kCli),
    };
    args << book_args("push", id);
    when_online(
        kWifiLeaveWaitS,
        [args, id] {
            log_line(QStringLiteral("push %1").arg(id));
            if (!QProcess::startDetached(QStringLiteral("/bin/sh"), args)) {
                log_line(QStringLiteral("push: could not start the CLI"));
            }
        },
        [] { log_line(QStringLiteral("push: nothing sent")); });
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
    {0},
};

static struct nh_dlsym NickelInkSyncDlsym[] = {
    {.name = "_ZN6Device16getCurrentDeviceEv", .out = nh_symoutptr(Device__getCurrentDevice), .desc = "Tolino check"},
    {.name = "_ZNK6Device8isTolinoEv", .out = nh_symoutptr(Device__isTolino), .desc = "Tolino check"},
    {.name = "_ZNK7Content5getIdERK7QString", .out = nh_symoutptr(Content__getId), .desc = "Book ID"},
    {.name = "_ZNK8Bookmark6isNullEv", .out = nh_symoutptr(Bookmark__isNull), .desc = "Opened without a bookmark"},
    {.name = "_ZN20MainWindowController14sharedInstanceEv", .out = nh_symoutptr(MainWindowController__sharedInstance), .desc = "Reading view"},
    {.name = "_ZNK20MainWindowController11currentViewEv", .out = nh_symoutptr(MainWindowController__currentView), .desc = "Reading view"},
    {.name = "_ZN11ReadingView26bookmarkForCurrentLocationEv", .out = nh_symoutptr(ReadingView__bookmarkForCurrentLocation), .desc = "Current position"},
    {.name = "_ZNK8Bookmark16getStartLocationEv", .out = nh_symoutptr(Bookmark__getStartLocation), .desc = "Current position"},
    {.name = "_ZNK15ContentLocation8toStringEv", .out = nh_symoutptr(ContentLocation__toString), .desc = "Current position"},
    {.name = "_ZN8BookmarkD1Ev", .out = nh_symoutptr(Bookmark__dtor), .desc = "Current position"},
    {.name = "_ZN11ReadingView12goToBookmarkERK8Bookmark", .out = nh_symoutptr(ReadingView__goToBookmark), .desc = "Jump"},
    {.name = "_ZN8Bookmark16setStartLocationERK15ContentLocation", .out = nh_symoutptr(Bookmark__setStartLocation), .desc = "Jump"},
    {.name = "_ZN15ContentLocation6createE7QStringS0_", .out = nh_symoutptr(ContentLocation__create), .desc = "Jump"},
    {.name = "_ZNK15ContentLocation7isValidEv", .out = nh_symoutptr(ContentLocation__isValid), .desc = "Jump"},
    {.name = "_ZN15ContentLocationD1Ev", .out = nh_symoutptr(ContentLocation__dtor), .desc = "Jump"},
    {.name = "_ZN23WirelessWorkflowManager14sharedInstanceEv", .out = nh_symoutptr(WirelessWorkflowManager__sharedInstance), .desc = "Wi-Fi", .optional = true},
    {.name = "_ZN23WirelessWorkflowManager20isInternetAccessibleEv", .out = nh_symoutptr(WirelessWorkflowManager__isInternetAccessible), .desc = "Wi-Fi", .optional = true},
    {.name = "_ZN23WirelessWorkflowManager15connectWirelessEbb", .out = nh_symoutptr(WirelessWorkflowManager__connectWireless), .desc = "Wi-Fi", .optional = true},
    {0},
};

NickelHook(
    .init  = nickelinksync_init,
    .info  = &NickelInkSync,
    .hook  = NickelInkSyncHook,
    .dlsym = NickelInkSyncDlsym,
)
