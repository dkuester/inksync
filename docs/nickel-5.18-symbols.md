# libnickel symbols for firmware 5.18 (Qt6)

What the NickelHook plugin (N4) needs from Nickel on the Tolino Shine
Color, firmware 5.18.264769: book opened, book closed, and a way to put a
pulled position into the book being opened.

## Sources

| | |
|---|---|
| Symbol list | `libnickel-5.18.syms.gz` from the owner's NickelTolinoShopRedirect repo: the defined dynamic symbols of `libnickel.so.1.0.0` from exactly the **Tolino** build 5.18.264769 (99,420 symbols). Every symbol below is in it. |
| Disassembly | `libnickel.so.1.0.0` from the **Kobo** build 5.18.270971 (46 MB, ARM32, stripped, dynamic symbols kept), the closest build at hand. Every symbol below is in both builds. |

Neither file is in this repository. The code is Thumb-2: `llvm-nm` prints
the addresses without the Thumb bit, so disassemble with
`llvm-objdump -d --triple=thumbv7a-linux-gnueabihf` from the even
address, for the size `llvm-nm -S` gives.

Calling convention (ARM EABI, Itanium C++): `this` in `r0`; a class
returned by value goes through a hidden result pointer in `r0`, and `this`
moves to `r1`. A by-value return shows as `mov r4, r0` at entry and
`mov r0, r4` before the return; `void` leaves `r0` untouched at the end.

## Result

| Purpose | Symbol | Signature (checked) |
|---|---|---|
| Book opened | `_ZN17ReadingController9setVolumeERK6VolumeRK8Bookmark` | `void ReadingController::setVolume(Volume const&, Bookmark const&)` |
| Book closed | `_ZN11ReadingView24leavingReadingExperienceEv` | `void ReadingView::leavingReadingExperience()` |
| Book ID | `_ZNK7Content5getIdERK7QString` | `QString Content::getId(QString const&) const` (by value; the argument is unused, pass `QString()`) |
| Copy the book | `_ZN6VolumeC1ERKS_`, `_ZN6VolumeD1Ev` | `Volume::Volume(Volume const&)`, `Volume::~Volume()` |
| Set the position | `_ZN7Content22setChapterIdBookmarkedERK8QVariant` | `void Content::setChapterIdBookmarked(QVariant const&)` (stored as `QByteArray`) |
| Set the percentage | `_ZN7Content14setPercentReadERK8QVariant` | `void Content::setPercentRead(QVariant const&)` (stored as `int`) |
| Book loaded again | `_ZN13VolumeManager7getByIdERK7QStringS2_` | `static Volume VolumeManager::getById(QString const& id, QString const&)` (by value: result pointer in `r0`, which it also returns; the arguments in `r1`, `r2`) |

How they connect, from the call graph of the whole library:

- **Opening.** `N3ReaderOpener::openReader(Context const&)` pushes the
  reading controller and calls `ReadingController::setVolume(volume,
  context.bookmark)`. That passes both on to `ReadingView::setVolume`,
  which compares the bookmark with `HasNull<Bookmark>::null()` and calls
  `ReadingView::setupReader(QString const&)`. **`setupReader` loads the
  book again from the database** (`VolumeManager::getById`, via the PLT)
  and sets up the reader state from that. On the Tolino the chapter to
  open comes from the `Volume` passed to `setVolume`, the place in the
  chapter from the one `getById` loads (device test below). A bookmark,
  if given (a link, a search result), takes precedence.
- **Closing.** `ReadingView::leavingReadingExperience()` calls
  `onLeaving()`, then `saveProgress()`, which goes through
  `updateContentLocation(…)` to `VolumeManager::updateBookmark(Volume&,
  QString const&, int, QDateTime const&, int)` and sets `DateLastRead`;
  after that it records the reading statistics. Calling the original first
  and pushing afterwards sees the saved position.
  `ReadingController::closeReadingView()` is no use here: only special
  cases call it (delete, preview bought, end of book view).
- **Page turns** go through `ReadingView::updateContentLocation(…)` →
  `VolumeManager::updateBookmark(…)`, not needed by the plugin.

## The pull path

What works on the Tolino, verified on the device, is
[step 4](#step-4-write-the-row-then-open): write the pulled position into
the book's row just before Nickel opens the book. Steps 1 to 3 below are
the ways tried before, kept with their device tests so nobody tries them
again.

**Renderer.** On the Tolino 5.18 Nickel renders a plain EPUB with
**TENG**, a web-engine renderer (`WebEngineRenderer`, `TengAPIHelper`,
pages under `/TENG/kobo/index.html`), not with Adobe's RMSDK. "RMSDK" in
`ContentLocation` is only the name of the `#point(/…:offset)` format
stored in `ChapterIDBookmarked`; Nickel converts it for TENG
(`ContentLocation::toTENGString`, `WebEngineRenderer::bookmarkForVolume`).
That is why `ReadingView::isBookOpened()` (a cast to another renderer
type) stayed false and why steps 1 to 3, which work on `ReadingView` and
the loaded `Volume`, did not move TENG. A TENG-level jump exists,
`TengAPIHelper::openBookmark(ContentLocation const&) const`, but reaching
the helper object needs field offsets; not used.

The first idea was to avoid writing the database while Nickel runs: Nickel keeps its
`Volume` in memory and writes it back on every page turn and on close
(`VolumeManager::updateBookmark`), and two processes would write the same
database. The plugin instead sets the pulled position in **both** `Volume`s
Nickel opens the book from:

1. a copy of the `Volume` handed to the original `setVolume` (the
   chapter), and
2. the `Volume` that `VolumeManager::getById` returns for this book while
   the original `setVolume` runs (the place in the chapter). A hook on
   `getById` does that; it changes nothing at other times or for other
   books.

The first version did only step 1:

```cpp
// Hooked: void ReadingController::setVolume(Volume const&, Bookmark const&)
extern "C" __attribute__((visibility("default")))
void _nh_set_volume(void *self, const Volume &volume, const Bookmark &bookmark) {
  QString id = Content__getId(&volume, QString());
  Pulled p = run_cli_pull(id);           // nickelinksync on the device, read-only
  if (!p.found || !Bookmark__isNull(&bookmark)) {
    ReadingController__setVolume(self, volume, bookmark);
    return;
  }
  alignas(8) char copy[VOLUME_SIZE];     // or heap; size from Volume::Volume()
  Volume__Volume_copy(copy, &volume);
  Content__setChapterIdBookmarked(copy, QVariant(p.position.toUtf8()));
  Content__setPercentRead(copy, QVariant(p.percent));
  ReadingController__setVolume(self, *(Volume *)copy, bookmark);
  Volume__dtor(copy);
}
```

**Device test of step 1 alone** (Shine Color 5.18, Book R, 2026-10-24):
KOReader was at R3-45 (`chapter3.xhtml#point(/1/4/92/1:0)`, 75 %), the
database still at R2-133 in chapter 2. The plugin pulled R3-45 and handed
Nickel the copy; Nickel opened **chapter 3 at its start (R3-1)**: the
chapter from the copy, but the place in the chapter from the row
`getById` had loaded, which pointed into chapter 2 and so did not fit.
Hence step 2 (`plugin/src/nickelinksync.cc`; the log says whether
`getById` was seen while opening). The `Volume` is 12 bytes (vtable,
shared data, flags; see its copy constructor). Nickel opens at the new
position and saves it itself on the next page turn and on close, so the
database never has to be written while Nickel runs.

**Device test of steps 1 and 2** (same day): KOReader at R3-179
(`chapter3.xhtml#point(/1/4/360/1:57)`), the database at R3-143 in the
same chapter. The log shows one `getById` while opening, from
`ReadingView::setupReader`, patched, and one a second later from
`ReadingController::currentlyOpenBook`. Nickel still stood at
`chapter3.xhtml#point(/1/4/2/1:0)`, the chapter heading, 3 s and 10 s
after opening: neither the pulled place nor the old one in the same
chapter. Changing the position in a loaded `Volume` gets the chapter
across, never the place in it.

### Step 3: jump once the book is open

So the plugin moves the open book with Nickel's own navigation. Polling
every 300 ms until the reading view is current and reports the same
location twice in a row, it takes Nickel's bookmark for the current
location, sets the pulled position as its start and calls `goToBookmark`.

A fifth device test then showed that waiting for Wi-Fi before opening is
the bigger problem: the connection took longer than the 12 s allowed, and
`connectWireless(false, false)` showed Nickel's network list over the
open reading view, which left it (and pushed). So the plugin now opens the
book at once and does everything else in the background: Wi-Fi
(`connectWireless(false, true)`, the silent variant that
`connectWirelessSilently()` calls; after 15 s the dialog variant, so the
reader can pick a network), then `pull`, then the jump. Steps 1 and 2 are
gone: the pulled position only arrives after opening, so the jump also has
to cross chapters.

| Purpose | Symbol | Signature (checked) |
|---|---|---|
| (not used) | `_ZNK11ReadingView12isBookOpenedEv` | `bool ReadingView::isBookOpened() const`: stayed false for 15 s with Book R open on the Tolino (it checks for one renderer type), so it cannot tell that a plain EPUB is open |
| Current view | `_ZN20MainWindowController14sharedInstanceEv`, `_ZNK20MainWindowController11currentViewEv` | `MainWindowController *sharedInstance()`, `QWidget *currentView() const` (object name `ReadingView`) |
| Current position | `_ZN11ReadingView26bookmarkForCurrentLocationEv` | `Bookmark ReadingView::bookmarkForCurrentLocation()` (by value; a `Bookmark` is 8 bytes: vtable, shared data) |
| Its start | `_ZNK8Bookmark16getStartLocationEv` | `ContentLocation const& Bookmark::getStartLocation() const`, a **reference** (`d + 0x104`), not a value |
| As text | `_ZNK15ContentLocation8toStringEv` | `QString ContentLocation::toString() const`: the same form as `ChapterIDBookmarked`, e.g. `OEBPS/text/chapter3.xhtml#point(/1/4/2/1:0)` |
| Build a location | `_ZN15ContentLocation6createE7QStringS0_` | `static ContentLocation create(QString position, QString spinePath)` (by value; a `ContentLocation` is a `shared_ptr`, 8 bytes). The format comes from the position string: `^([^:#]+)(#point.+)$` is "RMSDK", parsed as `^([^#]+)(#[^:]+):(\d+)\)$`; `spinePath` is only needed for CFI |
| Valid? | `_ZNK15ContentLocation7isValidEv` | `bool ContentLocation::isValid() const` |
| Set it | `_ZN8Bookmark16setStartLocationERK15ContentLocation` | `void Bookmark::setStartLocation(ContentLocation const&)` (detaches; `handleSetLocation` also sets container path and offset) |
| Jump | `_ZN11ReadingView12goToBookmarkERK8Bookmark` | `bool ReadingView::goToBookmark(Bookmark const&)` (finds the chapter through `KEPubUtility::shortcoverForBookmark`) |
| Empty bookmark | `_ZNK8Bookmark6isNullEv` | `bool Bookmark::isNull() const` |

All optional in the plugin: without them it opens as before and logs why.

**Device tests of step 3** (same day, after opening at once and pulling
in the background): from chapter 2 to chapter 1, and within chapter 2
from R2-121 to R2-170. Both times `goToBookmark` returned true and
Nickel stayed where it was (`bookmarkForCurrentLocation` 2 s later).
`ReadingController::followLink(Bookmark const&)` goes the same way
(`shortcoverForBookmark`, then `goToBookmark(Shortcover, Bookmark)`), and
the reading menu's scrubber jumps by page number through the reader, not
by bookmark. A changed bookmark does not move the open book.

### Step 4: write the row, then open

What did work is the USB write-back: a consistent book row in the
database, loaded by Nickel. So the plugin does that while Nickel runs,
just before it opens the book: it waits for Wi-Fi (silently, at most
10 s, in a local event loop), runs `pull --live --write --emit` for that
one book, which writes the position into the row through SQLite's normal
locking (one backup a day with SQLite's backup API), and hands the
original `setVolume` a copy of the `Volume` with the same position, so the
chapter (from the `Volume`) and the place in it (from the row that
`setupReader` loads through `getById`) agree. Three seconds later it checks
that Nickel stands in the pulled chapter. A book opened without a pull, or
not in the pulled chapter, is not pushed when it is left: in the device
tests that push overwrote the newer position KOReader had sent.

**Device test, verified** (Shine Color 5.18, Book R, 2026-10-24, tenth
run): KOReader at R3-49 (`p[49]/text()[1].98`, 76 %, on kosync.eu at
16:08:00 UTC), the Tolino at R2-121, last read 16:04 UTC. Opening the book
pulled and wrote `chapter3.xhtml#point(/1/4/100/1:101)`, made the day's
backup (integrity check ok, holding the old position), and Nickel opened
on the page with that point (its top in R3-48;
`#point(/1/4/98/1:44)` 4 s later). After reading on to R3-58 and leaving,
the push sent `p[58]/text()[1].41` (75 %) and KOReader showed it at the
bottom of its page. No journal files were left next to the database.

## The push path

After the original `leavingReadingExperience()` the position is saved,
so the hook runs `nickelinksync push` for that book (in the background,
`QProcess::startDetached`, so leaving the book never waits for the
network). The CLI reads the database while Nickel runs, so on the device
it must open it with `mode=ro`, **not** `immutable=1`: the database is in
WAL mode, and `immutable=1` ignores what is still in
`KoboReader.sqlite-wal`, which is where Nickel's latest write is (K1).

An alternative without hooks, as NickelStorygraph does it: watch
`QStackedWidget::currentChanged` on the main window and react when the
view named `ReadingView` is left. The hook is more direct and fires
exactly once per close; the signal is the fallback if the hook turns out
to fire too early.

## Kobo 4.46

Not checked yet (Kobo pass, N1). Signatures change between Qt5 and Qt6
(for example `Content::getId` has no argument on 4.x), so every symbol
above has to be checked again in the 4.46 `libnickel`.

## Messages on the screen

The plugin shows each outcome of a pull or push as one of Nickel's toasts,
the short boxes Nickel itself shows (e.g. after a rating).

| Purpose | Symbol | Signature (checked) |
|---|---|---|
| Toast | `_ZN20MainWindowController5toastERK7QStringS2_i` | `void MainWindowController::toast(QString const& primary, QString const& secondary, int ms)` |

The function is two instructions: `adds r0, #0x14` and a tail call to
`N3ToasterController::queueMeSomeToast(QString const&, QString const&,
int)`, so `this` is the `MainWindowController` (from `sharedInstance()`)
and the controller at offset `0x14` queues the toast. Nothing writes
`r0` for a result, and callers ignore it: `void`. `queueMeSomeToast`
builds a `QueuedToast` (two `QString`s and the `int`) and, unless the
queue is full (`ApplicationSettings::numMaxToasts()`), shows it through
`N3ToasterController::displayToast`, which creates an `N3ToastView` with
both lines, places it in `uiGeometry()` and starts a `QTimer` with
`QueuedToast::getDuration()`: the `int` is how long the toast stays, in
milliseconds. Nickel's own `RatingControls::showFeedbackToast` passes
1500. The plugin uses 3000 for results and 5000 for problems. Optional:
without it the plugin works as before, silently.

The texts are German only for now: no way was found yet to tell the
device language with certainty. The plugin logs `QLocale()` and
`QLocale::system()` at start, so a device test can show whether they
follow Nickel's language setting (`MainWindowController::updateLocale
(QLocale const&)` exists).
