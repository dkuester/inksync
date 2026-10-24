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
 *   both places Nickel takes it from: a copy of the Volume handed to
 *   setVolume (the chapter), and the Volume that ReadingView::setupReader
 *   loads again from the database through VolumeManager::getById (the
 *   place in the chapter). The getById hook only changes that one book, and
 *   only while the original setVolume runs. Nothing is written to the
 *   database; Nickel opens at the position and saves it itself. Only for
 *   sideloaded books opened without a bookmark (not a link or a search
 *   result), and with a time limit so a missing network never blocks for
 *   long.
 *
 * Leaving (ReadingView::leavingReadingExperience()):
 *   The original saves the position (saveProgress); after it returns, the CLI
 *   pushes that book in the background (`push --live`), so leaving the book
 *   never waits for the network.
 *
 * Symbols and signatures: docs/nickel-5.18-symbols.md.
 *
 * Tolino only for now: init() leaves the plugin inactive on a Kobo (the
 * position formats differ, see SCOPE.md), and without the CLI or config.ini.
 * Log: NickelInkSync.log next to the CLI.
 */

#include <NickelHook.h>

#include <QByteArray>
#include <QDateTime>
#include <QFile>
#include <QFileInfo>
#include <QProcess>
#include <QString>
#include <QStringList>
#include <QVariant>

typedef void Device;
typedef void Volume;
typedef void Bookmark;
typedef void ReadingController;
typedef void ReadingView;

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
    const Pulled p = pull(id);
    if (!p.found) {
        ReadingController__setVolume(self, volume, bookmark);
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
}

extern "C" __attribute__((visibility("default")))
Volume *_nh_VolumeManager__getById(Volume *out, const QString *id, const QString *other)
{
    VolumeManager__getById(out, id, other);
    if (g_pending.active && *id == g_pending.id) {
        Content__setChapterIdBookmarked(out, QVariant(g_pending.position));
        Content__setPercentRead(out, QVariant(g_pending.percent));
        if (!g_pending.applied) {
            log_line(QStringLiteral("open %1: position set in the book Nickel loaded").arg(*id));
        }
        g_pending.applied = true;
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
    log_line(QStringLiteral("push %1").arg(g_open_id));
    if (!QProcess::startDetached(QStringLiteral("/bin/sh"), args)) {
        log_line(QStringLiteral("push: could not start the CLI"));
    }
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
    {0},
};

NickelHook(
    .init  = nickelinksync_init,
    .info  = &NickelInkSync,
    .hook  = NickelInkSyncHook,
    .dlsym = NickelInkSyncDlsym,
)
