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
 *   Before Nickel opens the book, the plugin switches Wi-Fi on (silently,
 *   at most kOpenWifiMs), asks the CLI (`pull --live --write --emit`), which
 *   writes a position to take into the book's row in the database Nickel
 *   holds open (SQLite locking, one backup a day), and hands Nickel a copy
 *   of the Volume with that position (Content::setChapterIdBookmarked,
 *   setPercentRead). Nickel takes the chapter from that Volume and the place
 *   in the chapter from the row it loads again (VolumeManager::getById in
 *   ReadingView::setupReader), so both have to agree. Only for sideloaded
 *   books opened without a bookmark (not a link or a search result).
 *
 *   Device tests showed the other ways fail on the Tolino: changing the
 *   loaded Volumes alone gets only the chapter across, and
 *   ReadingView::goToBookmark with a changed bookmark leaves Nickel where it
 *   is (docs/nickel-5.18-symbols.md).
 *
 * Leaving (ReadingView::leavingReadingExperience()):
 *   The original saves the position (saveProgress); after it returns, the CLI
 *   pushes that book in the background (`push --live`), once Wi-Fi is up. Not
 *   while the background pull of that book still runs: then the reading view
 *   was only covered (a dialog), and its position is about to change. And
 *   not when the book was opened without a pull (no network), or Nickel did
 *   not open the pulled chapter: the device position would then overwrite a
 *   newer one on the server (device test).
 *
 * Network: Nickel switches Wi-Fi off when idle. The plugin asks it to connect
 * without a dialog (WirelessWorkflowManager::connectWireless(false, true),
 * which is what connectWirelessSilently() calls). Opening waits for it in a
 * local event loop, so the UI keeps running. Leaving does not wait; if the
 * silent attempt brings no network within kSilentWifiS, it asks again with
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
#include <QEventLoop>
#include <QFile>
#include <QFileInfo>
#include <QProcess>
#include <QProcessEnvironment>
#include <QString>
#include <QVariant>
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
static const char kBackups[] = "/mnt/onboard/.adds/NickelInkSync/backups";
static const char kOnboard[] = "/mnt/onboard";
static const char kSideloaded[] = "file:///mnt/onboard/";
static const qint64 kLogMaxBytes = 128 * 1024;
// Seconds per request for pull, and how long opening may wait for it as a
// whole (CLI start, database, backup, hash, two requests).
static const char kPullTimeout[] = "6";
static const int kPullWaitMs = 15000;
// How long opening waits for Wi-Fi; how long leaving waits in the
// background, the first kSilentWifiS without a dialog.
static const int kOpenWifiMs = 10000;
static const int kWifiLeaveWaitS = 60;
static const int kSilentWifiS = 15;
// A Volume is 12 bytes in 5.18 (vtable, shared data, flags; see the copy
// constructor), a Bookmark 8 (vtable, shared data). Room to spare.
static const size_t kVolumeBytes = 64;
static const size_t kBookmarkBytes = 64;
// When to check that Nickel opened the pulled chapter.
static const int kCheckOpenMs = 3000;

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
static void (*Volume__copy)(Volume *, const Volume *);  // Volume(Volume const&)
static void (*Volume__dtor)(Volume *);
static void (*Content__setChapterIdBookmarked)(Volume *, const QVariant &);
static void (*Content__setPercentRead)(Volume *, const QVariant &);
static MainWindowController *(*MainWindowController__sharedInstance)();
// Returns a QWidget*; QObject is its first base, so the pointer is the same.
static QObject *(*MainWindowController__currentView)(const MainWindowController *);
// Bookmark by value: hidden result pointer in r0, `this` in r1.
static void (*ReadingView__bookmarkForCurrentLocation)(void *result, ReadingView *);
// Returns a reference (d + 0x104), not a value.
static const ContentLocation *(*Bookmark__getStartLocation)(const Bookmark *);
static QString (*ContentLocation__toString)(const ContentLocation *);
static void (*Bookmark__dtor)(Bookmark *);

// Optional (NULL if missing): without them the plugin tries without Wi-Fi.
static WirelessWorkflowManager *(*WirelessWorkflowManager__sharedInstance)();
static bool (*WirelessWorkflowManager__isInternetAccessible)(WirelessWorkflowManager *);
static void (*WirelessWorkflowManager__connectWireless)(WirelessWorkflowManager *, bool, bool);

// Set at the end of a successful init(); every hook passes through until then.
static bool g_active = false;
// ContentID of the book opened last (GUI thread only).
static QString g_open_id;
// The book that was not brought in line with the server when it was
// opened, or whose pulled chapter Nickel did not open: not pushed.
static QString g_not_synced;

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

// Waits until the network is there, at most `ms`, in a local event loop so
// the UI keeps running; connects silently (no dialog over the book).
static bool wait_online(int ms)
{
    if (online()) {
        return true;
    }
    if (!connect_wifi(true)) {
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

// After opening: Nickel shows a page start, which lies before the pulled
// point, so "reached" means the same chapter.
static void check_open(const QString &id, const QString &target)
{
    ReadingView *view = reading_view();
    if (!view || g_open_id != id) {
        log_line(QStringLiteral("after opening: not in the reading view of the book"));
        return;
    }
    const QString at = current_location(view);
    if (chapter_of(at) == chapter_of(target)) {
        log_line(QStringLiteral("after opening: Nickel is at %1, in the pulled chapter").arg(at));
        if (g_not_synced == id) {
            g_not_synced.clear();
        }
    } else {
        log_line(QStringLiteral("after opening: Nickel is at %1, not in the pulled chapter; "
                                "leaving this book will not push").arg(at));
    }
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

// Runs `pull --live --write --emit` for one book and waits for it: the CLI
// writes a position to take into the database and prints it as
// `NICKELINKSYNC_POSITION<tab>percent<tab>position`. Found only if it also
// wrote it (exit 0).
static Pulled pull_and_write(const QString &id)
{
    Pulled result;
    QStringList args = book_args("pull", id);
    args << QStringLiteral("--write") << QStringLiteral("--emit")
         << QStringLiteral("--timeout") << QString::fromLatin1(kPullTimeout);
    QProcess p;
    QProcessEnvironment env = QProcessEnvironment::systemEnvironment();
    env.insert(QStringLiteral("NICKELINKSYNC_BACKUP_DIR"), QString::fromLatin1(kBackups));
    p.setProcessEnvironment(env);
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
    if (p.exitStatus() != QProcess::NormalExit || p.exitCode() != 0) {
        return result;
    }
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
    g_not_synced.clear();
    if (!id.startsWith(QString::fromLatin1(kSideloaded))) {
        ReadingController__setVolume(self, volume, bookmark);
        return;
    }
    if (!Bookmark__isNull(bookmark)) {
        log_line(QStringLiteral("open %1 with a bookmark (link, search): not pulled").arg(id));
        ReadingController__setVolume(self, volume, bookmark);
        return;
    }
    // Until the server's position is known, this book is not in line with it.
    g_not_synced = id;
    if (!wait_online(kOpenWifiMs)) {
        log_line(QStringLiteral("open %1 unchanged: no network; leaving it will not push").arg(id));
        ReadingController__setVolume(self, volume, bookmark);
        return;
    }
    const Pulled p = pull_and_write(id);
    if (!p.found) {
        // Nothing to take (or the server could not be asked: then still not
        // in line, see pull_and_write's log line).
        ReadingController__setVolume(self, volume, bookmark);
        return;
    }
    log_line(QStringLiteral("open %1 at %2 (%3 %)").arg(id, QString::fromUtf8(p.position)).arg(p.percent));
    alignas(8) unsigned char copy[kVolumeBytes];
    Volume__copy(copy, volume);
    Content__setChapterIdBookmarked(copy, QVariant(p.position));
    Content__setPercentRead(copy, QVariant(p.percent));
    ReadingController__setVolume(self, copy, bookmark);
    Volume__dtor(copy);
    const QString target = QString::fromUtf8(p.position);
    QTimer::singleShot(kCheckOpenMs, qApp, [id, target] { check_open(id, target); });
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
    if (g_not_synced == id) {
        log_line(QStringLiteral("left %1, which is not in line with the server: not pushed").arg(id));
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
    {.name = "_ZN6VolumeC1ERKS_", .out = nh_symoutptr(Volume__copy), .desc = "Copy of the book to open"},
    {.name = "_ZN6VolumeD1Ev", .out = nh_symoutptr(Volume__dtor), .desc = "Copy of the book to open"},
    {.name = "_ZN7Content22setChapterIdBookmarkedERK8QVariant", .out = nh_symoutptr(Content__setChapterIdBookmarked), .desc = "Pulled position"},
    {.name = "_ZN7Content14setPercentReadERK8QVariant", .out = nh_symoutptr(Content__setPercentRead), .desc = "Pulled percentage"},
    {.name = "_ZN20MainWindowController14sharedInstanceEv", .out = nh_symoutptr(MainWindowController__sharedInstance), .desc = "Reading view"},
    {.name = "_ZNK20MainWindowController11currentViewEv", .out = nh_symoutptr(MainWindowController__currentView), .desc = "Reading view"},
    {.name = "_ZN11ReadingView26bookmarkForCurrentLocationEv", .out = nh_symoutptr(ReadingView__bookmarkForCurrentLocation), .desc = "Current position"},
    {.name = "_ZNK8Bookmark16getStartLocationEv", .out = nh_symoutptr(Bookmark__getStartLocation), .desc = "Current position"},
    {.name = "_ZNK15ContentLocation8toStringEv", .out = nh_symoutptr(ContentLocation__toString), .desc = "Current position"},
    {.name = "_ZN8BookmarkD1Ev", .out = nh_symoutptr(Bookmark__dtor), .desc = "Current position"},
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
