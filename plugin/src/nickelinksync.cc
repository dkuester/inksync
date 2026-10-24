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
 *   pushes that book in the background (`push --live --emit`, an
 *   asynchronous QProcess whose output goes to the log), once Wi-Fi is up.
 *   Leaving never waits for it. Not
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
 * Messages: each outcome is shown as one of Nickel's toasts
 * (MainWindowController::toast), German only for now; `notify` in config.ini
 * picks which: `all` (default), `errors` (only what went wrong or was left
 * undone) or `none`. The CLI names the outcome in `NICKELINKSYNC_…` lines.
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
#include <QLocale>
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
static const char kPullTimeout[] = "5";
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
// Toasts: how long they stay (Nickel's own use 1500 ms), and how long after
// setVolume the one about opening is queued, once the reading view is up.
static const int kToastMs = 3000;
static const int kProblemToastMs = 5000;
static const int kOpenToastDelayMs = 1000;
// Seconds per request for push, and when the plugin gives up on it.
static const char kPushTimeout[] = "15";
static const int kPushWaitMs = 60000;

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
// Optional: without it, no messages. Queues a toast with a primary and a
// secondary line for `ms` milliseconds; returns nothing (a tail call to
// N3ToasterController::queueMeSomeToast, see docs/nickel-5.18-symbols.md).
static void (*MainWindowController__toast)(MainWindowController *, const QString &, const QString &, int);

// `notify` in config.ini.
enum class Notify { All, Errors, None };
static Notify g_notify = Notify::All;

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

// Reads `notify` from config.ini (the CLI checks the value too); `all`
// if it is missing or not understood.
static Notify read_notify()
{
    QFile f(QString::fromLatin1(kConfig));
    if (!f.open(QIODevice::ReadOnly | QIODevice::Text)) {
        return Notify::All;
    }
    Notify notify = Notify::All;
    for (const QByteArray &raw : f.readAll().split('\n')) {
        const QByteArray line = raw.trimmed();
        const int eq = line.indexOf('=');
        if (line.startsWith('#') || line.startsWith(';') || eq < 0 || line.left(eq).trimmed() != "notify") {
            continue;
        }
        const QByteArray value = line.mid(eq + 1).trimmed();
        if (value == "errors") {
            notify = Notify::Errors;
        } else if (value == "none") {
            notify = Notify::None;
        } else {
            if (value != "all") {
                log_line(QStringLiteral("notify = %1 is not all, errors or none: using all").arg(QString::fromUtf8(value)));
            }
            notify = Notify::All;
        }
    }
    return notify;
}

// Shows one of Nickel's toasts after `delay_ms`, if `notify` allows it: a
// `problem` (something went wrong or was left undone) unless `none`, any
// other only with `all`.
static void toast(bool problem, const QString &primary, const QString &secondary, int delay_ms = 0)
{
    if (!MainWindowController__toast || g_notify == Notify::None || (g_notify == Notify::Errors && !problem)) {
        return;
    }
    log_line(QStringLiteral("toast: %1 / %2").arg(primary, secondary));
    QTimer::singleShot(delay_ms, qApp, [problem, primary, secondary] {
        MainWindowController__toast(MainWindowController__sharedInstance(), primary, secondary,
                                    problem ? kProblemToastMs : kToastMs);
    });
}

// The fields of each `NICKELINKSYNC_<name>` line in the CLI's output.
static QList<QList<QByteArray>> machine_lines(const QByteArray &out, const char *name)
{
    const QByteArray prefix = QByteArray("NICKELINKSYNC_") + name;
    QList<QList<QByteArray>> found;
    for (const QByteArray &line : out.split('\n')) {
        QList<QByteArray> f = line.trimmed().split('\t');
        if (f[0] == prefix) {
            f.removeFirst();
            found << f;
        }
    }
    return found;
}

// Why the server could not be asked, from `NICKELINKSYNC_ERROR<tab>kind<tab>
// message`, for the secondary line.
static QString error_reason(const QByteArray &out)
{
    const QList<QList<QByteArray>> errors = machine_lines(out, "ERROR");
    const QByteArray kind = errors.isEmpty() ? QByteArray() : errors[0].value(0);
    if (kind == "timeout") {
        return QStringLiteral("Der Server hat nicht rechtzeitig geantwortet");
    }
    if (kind == "network") {
        return QStringLiteral("Keine Verbindung zum Server");
    }
    if (kind == "auth") {
        return QStringLiteral("Anmeldung abgelehnt: Benutzer und Passwort prüfen");
    }
    if (kind == "server") {
        return QStringLiteral("Unerwartete Antwort vom Server");
    }
    return QStringLiteral("Fehler, siehe NickelInkSync.log");
}

// Why a position was not taken or sent, from the reason field of
// `NICKELINKSYNC_NOT_TAKEN` / `NICKELINKSYNC_NOT_SENT`.
static QString skip_reason(const QByteArray &reason)
{
    if (reason == "server-newer") {
        return QStringLiteral("Der Server hat einen neueren Stand");
    }
    if (reason == "not-translated") {
        return QStringLiteral("Die Position vom Server passt nicht zu diesem Buch");
    }
    return QStringLiteral("Buch oder Position auf dem Gerät nicht lesbar");
}

static const QString kUnansweredReason = QStringLiteral("Braucht eine Bestätigung (confirm in config.ini)");

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
        toast(true, QStringLiteral("Fortschritt nicht übernommen"),
              QStringLiteral("Nickel hat ein anderes Kapitel geöffnet"));
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
    // The CLI ran and finished with exit 0 (the server answered).
    bool ok = false;
    // Why not, for the message.
    QString error;
    // A position would need an answer the plugin cannot give.
    bool unanswered = false;
    // The server's position cannot be taken (NICKELINKSYNC_NOT_TAKEN): why.
    QByteArray not_taken;
    bool found = false;
    int percent = 0;
    QByteArray position;
    // Who stored it on the server.
    QString from;
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
        result.error = QStringLiteral("NickelInkSync konnte nicht starten");
        return result;
    }
    if (!p.waitForFinished(kPullWaitMs)) {
        p.kill();
        p.waitForFinished(1000);
        log_line(QStringLiteral("pull: no answer within %1 s, book opens unchanged").arg(kPullWaitMs / 1000));
        result.error = QStringLiteral("Keine Antwort innerhalb von %1 s").arg(kPullWaitMs / 1000);
        return result;
    }
    const QByteArray out = p.readAll();
    log_line(QStringLiteral("pull %1 (exit %2):\n%3").arg(id).arg(p.exitCode()).arg(QString::fromUtf8(out).trimmed()));
    if (p.exitStatus() != QProcess::NormalExit || p.exitCode() != 0) {
        result.error = error_reason(out);
        return result;
    }
    result.ok = true;
    result.unanswered = !machine_lines(out, "UNANSWERED").isEmpty();
    const QList<QList<QByteArray>> not_taken = machine_lines(out, "NOT_TAKEN");
    if (!not_taken.isEmpty()) {
        result.not_taken = not_taken[0].value(0);
    }
    for (const QList<QByteArray> &f : machine_lines(out, "POSITION")) {
        bool ok = false;
        const int percent = f.value(0).toInt(&ok);
        if (ok && f.size() == 2 && !f[1].isEmpty()) {
            result.found = true;
            result.percent = percent;
            result.position = f[1];
        }
    }
    const QList<QList<QByteArray>> from = machine_lines(out, "FROM");
    if (!from.isEmpty()) {
        result.from = QString::fromUtf8(from[0].value(0));
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
    // isInternetAccessible() only reports Nickel's own connectivity check:
    // it stayed false for 10 s with Wi-Fi connected (device test). So it
    // only decides how long to wait; the CLI always tries, and fails fast
    // without a network.
    if (!wait_online(kOpenWifiMs)) {
        log_line(QStringLiteral("Nickel reports no internet; trying anyway"));
    }
    const Pulled p = pull_and_write(id);
    if (!p.ok) {
        log_line(QStringLiteral("open %1 unchanged: the server could not be asked; leaving it will not push").arg(id));
        ReadingController__setVolume(self, volume, bookmark);
        toast(true, QStringLiteral("Fortschritt konnte nicht vom Server gelesen werden"), p.error, kOpenToastDelayMs);
        return;
    }
    if (p.unanswered) {
        log_line(QStringLiteral("open %1 unchanged: the position needs an answer (confirm); leaving it will not push").arg(id));
        ReadingController__setVolume(self, volume, bookmark);
        toast(true, QStringLiteral("Fortschritt nicht übernommen"), kUnansweredReason, kOpenToastDelayMs);
        return;
    }
    if (!p.not_taken.isEmpty()) {
        log_line(QStringLiteral("open %1 unchanged: the server's position cannot be taken; leaving it will not push").arg(id));
        ReadingController__setVolume(self, volume, bookmark);
        toast(true, QStringLiteral("Fortschritt nicht übernommen"), skip_reason(p.not_taken), kOpenToastDelayMs);
        return;
    }
    if (!p.found) {
        // The server holds nothing newer: the device is in line with it.
        g_not_synced.clear();
        ReadingController__setVolume(self, volume, bookmark);
        toast(false, QStringLiteral("Fortschritt ist aktuell"),
              QStringLiteral("Der Server hat keinen neueren Stand"), kOpenToastDelayMs);
        return;
    }
    log_line(QStringLiteral("open %1 at %2 (%3 %)").arg(id, QString::fromUtf8(p.position)).arg(p.percent));
    alignas(8) unsigned char copy[kVolumeBytes];
    Volume__copy(copy, volume);
    Content__setChapterIdBookmarked(copy, QVariant(p.position));
    Content__setPercentRead(copy, QVariant(p.percent));
    ReadingController__setVolume(self, copy, bookmark);
    Volume__dtor(copy);
    toast(false, QStringLiteral("Fortschritt vom Server übernommen"),
          p.from.isEmpty() ? QStringLiteral("%1 %").arg(p.percent)
                           : QStringLiteral("%1 %, von %2").arg(p.percent).arg(p.from),
          kOpenToastDelayMs);
    const QString target = QString::fromUtf8(p.position);
    QTimer::singleShot(kCheckOpenMs, qApp, [id, target] { check_open(id, target); });
}

// What a finished push means, for the log and the message.
static void push_finished(const QString &id, QProcess *p)
{
    const QByteArray out = p->readAll();
    log_line(QStringLiteral("push %1 (exit %2):\n%3").arg(id).arg(p->exitCode()).arg(QString::fromUtf8(out).trimmed()));
    const QString failed = QStringLiteral("Fortschritt konnte nicht gesendet werden");
    const QString not_sent = QStringLiteral("Fortschritt nicht gesendet");
    if (p->exitStatus() != QProcess::NormalExit || p->exitCode() != 0) {
        toast(true, failed, error_reason(out));
        return;
    }
    const QList<QList<QByteArray>> sent = machine_lines(out, "SENT");
    const QList<QList<QByteArray>> skipped = machine_lines(out, "NOT_SENT");
    if (!sent.isEmpty()) {
        toast(false, QStringLiteral("Fortschritt zum Server gesendet"),
              QStringLiteral("%1 %").arg(QString::fromUtf8(sent[0].value(0))));
    } else if (!skipped.isEmpty()) {
        toast(true, not_sent, skip_reason(skipped[0].value(0)));
    } else if (!machine_lines(out, "UNANSWERED").isEmpty()) {
        toast(true, not_sent, kUnansweredReason);
    } else if (!machine_lines(out, "IN_SYNC").isEmpty()) {
        toast(false, QStringLiteral("Fortschritt ist aktuell"), QStringLiteral("Der Server hat schon diesen Stand"));
    }
}

// Starts the push of one book and returns at once; its output goes to the
// log when it finishes, and the outcome to a toast.
static void start_push(const QString &id)
{
    log_line(QStringLiteral("push %1").arg(id));
    QStringList args = book_args("push", id);
    args << QStringLiteral("--emit") << QStringLiteral("--timeout") << QString::fromLatin1(kPushTimeout);
    QProcess *p = new QProcess(qApp);
    p->setProcessChannelMode(QProcess::MergedChannels);
    QObject::connect(p, &QProcess::finished, p, [id, p](int, QProcess::ExitStatus) {
        push_finished(id, p);
        p->deleteLater();
    });
    QObject::connect(p, &QProcess::errorOccurred, p, [p](QProcess::ProcessError error) {
        if (error == QProcess::FailedToStart) {
            log_line(QStringLiteral("push: could not start the CLI: %1").arg(p->errorString()));
            toast(true, QStringLiteral("Fortschritt konnte nicht gesendet werden"),
                  QStringLiteral("NickelInkSync konnte nicht starten"));
            p->deleteLater();
        }
    });
    // A push that hangs is ended; finished() then reports it.
    QTimer::singleShot(kPushWaitMs, p, [p] {
        if (p->state() != QProcess::NotRunning) {
            log_line(QStringLiteral("push: no answer within %1 s, ended").arg(kPushWaitMs / 1000));
            p->kill();
        }
    });
    p->start(QString::fromLatin1(kCli), args);
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
        toast(true, QStringLiteral("Fortschritt nicht gesendet"), QStringLiteral("Beim Öffnen nicht abgeglichen"));
        return;
    }
    when_online(
        kWifiLeaveWaitS,
        [id] { start_push(id); },
        [] {
            log_line(QStringLiteral("push: nothing sent"));
            toast(true, QStringLiteral("Fortschritt konnte nicht gesendet werden"), QStringLiteral("Kein WLAN"));
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
    g_notify = read_notify();
    // The messages are German; the locale is logged to learn whether the
    // device language can be told from it.
    log_line(QStringLiteral("active, CLI and configuration in %1; notify = %2, toasts %3, locale %4 (system %5)")
                 .arg(QString::fromLatin1(kDir))
                 .arg(QLatin1String(g_notify == Notify::All ? "all" : g_notify == Notify::Errors ? "errors" : "none"))
                 .arg(QLatin1String(MainWindowController__toast ? "available" : "missing"))
                 .arg(QLocale().name(), QLocale::system().name()));
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
    {.name = "_ZN20MainWindowController5toastERK7QStringS2_i", .out = nh_symoutptr(MainWindowController__toast), .desc = "Messages", .optional = true},
    {0},
};

NickelHook(
    .init  = nickelinksync_init,
    .info  = &NickelInkSync,
    .hook  = NickelInkSyncHook,
    .dlsym = NickelInkSyncDlsym,
)
