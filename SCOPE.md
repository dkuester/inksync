# Scope

**NickelInkSync** syncs the **reading progress of sideloaded ebooks** between
Kobo and Tolino e-readers (and KOReader) through a
[koreader-sync-server](https://github.com/koreader/koreader-sync-server)
(KOSync protocol).

Status: scope fixed, nothing implemented yet. Findings marked *verified* were
checked against real device databases (see [Verified findings](#verified-findings));
everything else is still an assumption.

## Goals

- Push the current position of a sideloaded book from a Kobo or Tolino to a
  KOSync server, and pull a newer position back onto the device.
- Interoperate with KOReader: a position written by NickelInkSync opens at the
  same place in KOReader, and the other way round.
- One code base for both device families. The Tolino Shine 5, Shine Color and
  Vision Color are built by Kobo and run Nickel with the same
  `.kobo/KoboReader.sqlite`, so they are treated as Kobo devices with a
  Tolino flag, not as a separate target.

## Supported

| | |
|---|---|
| Devices | Kobo on firmware 4.x (Qt5), Tolino Shine 5, Shine Color and Vision Color on firmware 5.x (Qt6) |
| Formats | kepub (`.kepub.epub`) and plain EPUB |
| Books | Sideloaded only (`ContentID` starting with `file:///mnt/onboard/`) |
| Server | Any KOSync-compatible server (koreader-sync-server, self-hosted forks) |

Both firmware lines are needed: the reference Kobo (Clara BW) runs 4.46, the
reference Tolino (Shine Color) runs 5.18. The Rust core is unaffected; the
NickelHook plugin is built twice (Qt5 and Qt6).

## Book identity

KOSync identifies a book by a hash of its file. **First version: the user puts
the identical file on every device**, and NickelInkSync uses KOReader's default
"binary" hash (MD5 over 1 KiB samples at offsets 0, 1024, 4096, 16384, …
up to 1 GiB, stopping at the end of the file; checked against the
`partial_md5_checksum` KOReader stored for every EPUB fixture). A kepub on one device and the source
EPUB on another are different books to the server.

Later: a manual mapping that links several file hashes to one book, so a
kepub and its source EPUB share progress (see the roadmap).
[BookOrbit](https://github.com/bookorbit/bookorbit) does this
([`docs/KOREADER_HASH_MATCHING.md`](https://github.com/bookorbit/bookorbit/blob/main/docs/KOREADER_HASH_MATCHING.md),
manual links take precedence over automatic hash matches) and has a position
converter for Kobo spans, XPointer and CFI
(`server/src/modules/position-converter/`).

## License

NickelInkSync is MIT-licensed. KOReader, koreader-sync-server and BookOrbit are
AGPL-3.0, so their code is a reference only: re-implement the documented
behaviour (KOSync protocol, partial MD5, XPointer format), never copy or
translate their source. NickelHook is MIT and can be used as is.

## Out of scope

- Kobo Store, Tolino/Thalia store and Onleihe books, and anything with DRM
- PDF, CBZ and other non-EPUB formats
- Highlights, notes and bookmarks (position only)
- Reading statistics
- A sync server of our own: NickelInkSync is a client only

## Architecture

```
┌──────────────── Kobo / Tolino ────────────────┐
│ Nickel ── NickelHook plugin (C++, Qt5 / Qt6)   │
│              │  book opened / closed           │
│              ▼                                 │
│        nickelinksync CLI (Rust, ARM) ──────────┼──► KOSync server ◄── KOReader
└────────────────────────────────────────────────┘          ▲
                                                            │
  Desktop: nickelinksync CLI (Rust) on a USB-mounted device ┘
```

### Rust core and CLI

One crate, built for the desktop (macOS, Linux, Windows) and cross-compiled
for the device. It owns all logic:

- **Device database access**: read positions from `KoboReader.sqlite`, opened
  read-only unless a write is explicitly requested.
- **Document identity**: compute the KOSync document hash for a book file.
- **Position translation**: Nickel position ↔ KOReader XPointer, using the
  EPUB itself (OPF spine, chapter files, Kobo span IDs, Adobe point paths).
- **KOSync client**: `users/auth`, `GET /syncs/progress/:document`,
  `PUT /syncs/progress`.
- **Conflict rule**: the newer side wins (device `DateLastRead` against the
  server's timestamp), with a confirmation set by `confirm` in the
  configuration: `never`, `backwards` (default: ask when the position that
  would be overwritten lies further on in the book than the new one) or
  `always` (ask whenever the positions differ). "Backwards" compares the
  place in the book, never the timestamps: spine index (`DocFragment[n]`)
  first, then, in the same chapter, the place in the text (paragraph, then
  offset, where the two XPointer paths can be compared), and the
  percentage only where neither decides. Each reader computes its own
  percentage: on the Tolino, Nickel's page start in R2-133 said 57 % while
  KOReader's place further on in R2-134 said 56 %. Not by time: Nickel sets `DateLastRead` when a book
  is merely opened, so newest-wins alone would let a device that was only
  opened overwrite a position read further on in KOReader. In a terminal
  the CLI asks "device or server?"; without one (the plugin, later) it
  overwrites nothing and reports the book. `--force` skips the question.

Desktop use: `nickelinksync push|pull|status --device /Volumes/KOBOeReader`.
On-device use: the same binary, called by the hook.

Device build: a statically linked `armv7-unknown-linux-musleabihf` binary
(`cargo-zigbuild`), independent of the device glibc, so one file serves Kobo
firmware 4.x and Tolino firmware 5.x. HTTPS to the KOSync server must use
`rustls`, not OpenSSL: no system library to link against or ship, and nothing
that breaks the static build. C dependencies such as SQLite are compiled in
(`rusqlite` with `bundled`).

### NickelHook plugin

Thin C++ layer, no sync logic of its own:

- On closing a book (or leaving the reading view): call the CLI to push.
- On opening a book: call the CLI to pull and, if a newer position exists,
  jump there through Nickel's own navigation instead of writing the database
  behind Nickel's back.
- Settings (server URL, user, device name) in `.adds/NickelInkSync/config.ini`,
  in the same format as the desktop configuration (see the README), passed
  to the CLI with `--config`.
- Packaging: `KoboRoot.tgz` for 4.x, `Kobo.tgz` for 5.x (hook in `imageformats/`).

## Verified findings

Reference devices, both read from a copy of the database, never written:

- **Kobo Clara BW**, firmware 4.46.23836, database version 176, two sideloaded kepubs
- **Tolino Shine Color**, firmware 5.18.264769, database version 220, two sideloaded plain EPUBs
- Both again after loading the placeholder EPUB fixtures (a kepub and two plain
  EPUBs), plus **KOReader 2026.07** (macOS) on the same files

| Question | Kobo 4.46 | Tolino 5.18 |
|---|---|---|
| Book row | `ContentType = 6`, `ContentID = file:///mnt/onboard/<path>` | same |
| MimeType | `application/x-kobo-epub+zip` (kepub), `application/epub+zip` (plain EPUB) | `application/epub+zip`, **also for a `.kepub.epub`** |
| Chapter rows | kepub: `ContentType = 9`, one per spine item, `ContentID = /mnt/onboard/<path>!<opf dir>!<href>`; plus `ContentType = 899`, one per ToC entry. Plain EPUB: `ContentType = 9`, one per **ToC entry**, `ContentID = file:///mnt/onboard/<path>#(<ToC index>)<target>`, target in `adobe_location` | `ContentType = 9`, one per **ToC entry** (no spine rows), `ContentID` is a **UUID**, target in `adobe_location` |
| Chapter → book | `BookID` = book `ContentID`, `VolumeIndex` = spine index (kepub, type 9), ToC index (plain EPUB) | `BookID` = book `ContentID`, `VolumeIndex` = **ToC index**, not spine index |
| Position field | `ChapterIDBookmarked`, `___PercentRead` (integer percent) | same |
| Position format | `<href>#kobo.N.M` (kepub span ID), `<href>#point(...)` (plain EPUB); the fragment can be missing (`<href>` only) | `<href>#point(/1/2/1/1:0)` (Adobe point), also for a kepub |
| `adobe_location` (book row) | empty, even for plain EPUB | empty, even for plain EPUB |
| Extra tables | — | `tolino_*` (cloud, DRM, Skoobe); `BookReader.sqlite` is encrypted and not needed |

In the chapter-row `ContentID`, `<href>` is relative to the OPF directory;
the chapter path in `ChapterIDBookmarked` and `adobe_location` is relative to
the EPUB root (`OEBPS/xhtml/…`). Never rely on the chapter-row `ContentID`
format: it differs between firmware lines. On the Kobo, `BookID` +
`VolumeIndex` of the `ContentType = 9` rows gives the spine item. On the
Tolino it does not: the rows are ToC entries (`NumShortcovers` counts them),
so in *Heat 2* `xhtml-0-81.xhtml` (spine index 81) has `VolumeIndex` 80, and
the *Titan 2* position lies in a spine item without a ToC entry and so
without a row. A plain EPUB on the Kobo 4.46 behaves the same way. Find the
chapter through the OPF spine there. All cases are in the
[test fixtures](tests/fixtures/README.md).

The fragment of `ChapterIDBookmarked` is optional: the Kobo stores the bare
chapter path at the start of a plain-EPUB chapter and on kepub pages without
`koboSpan` spans.

**Kepub span → XPointer** (Kobo): `OEBPS/xhtml/xhtml-0-81.xhtml#kobo.1.172`
→ spine index 81 → KOReader `DocFragment[82]`; span `id="kobo.1.172"` exists
in the chapter file. Resolve the span by its ID; don't derive
paragraph/sentence from `N.M` (calibre's kepub output numbers differently from
kepubify).

**Adobe point → DOM node** (Tolino): `point(/a/b/c…:offset)` is a path of
1-based indices over **all** child nodes, whitespace text nodes included,
where `/1` is the `html` element; `offset` is a **UTF-8 byte offset** into
the final text node (a no-break space or soft hyphen counts 2, so does every
umlaut), with whitespace counted as is. Comments differ by firmware: Kobo
4.46 skips them and joins the text around them, Tolino 5.18 counts them as
nodes. Checked on two books (`/1/2/1/1:0` hits the chapter heading in
*Heat 2*, `/1/4/16/1:356` mid-paragraph text in *Star Trek Titan 2*) and with
highlights in the probe book Book P on both devices.

**Offsets inside a text node** (highlights of the same eight words in the
probe books Book P and Book Q on every device, see the
[test fixtures](tests/fixtures/README.md)): node paths agree
everywhere; only the offsets need converting.

| | Unit | Whitespace | Comment |
|---|---|---|---|
| Kobo span (`StartOffset`) | UTF-16 units | as is | — (spans split there) |
| Adobe point, Kobo 4.46 | UTF-8 bytes | as is | skipped, text joined |
| Adobe point, Tolino 5.18 | UTF-8 bytes | as is | counted as a node |
| KOReader XPointer | code points | collapsed (`\n    ` = 1, leading spaces = 1) | splits `text()` |
| KOReader CFI | UTF-16 units | collapsed | skipped, text joined |

**XPointer → Adobe point** (back to the Tolino, for `pull`): walk the
XPointer below `DocFragment[n]` through spine item *n*−1 (elements by
tag name, `text()[m]` among text and CDATA nodes, comments skipped), then
count all child nodes from `/1` = `html` for the point. KOReader's offset
counts a run of whitespace as one character; going back, an offset after
such a run lands behind the whole run, on the next visible character. A
path that ends on an element, or does not resolve, gives the first
non-whitespace text (of the element, or of the chapter). All 16 Tolino
highlights in Book P and Book Q come back unchanged, KOReader's highlights
of the same words give exactly the Tolino's points, and so do the Book R
reading positions. Not done yet: back to the Kobo (kepub span, and the
4.x point that joins text around comments).

CDATA behaves like a comment in the Adobe point (skipped on Kobo 4.46, a
node on Tolino 5.18), is a `text()` node of its own in KOReader's XPointer,
and is joined into the text by KOReader's CFI. A character outside the BMP
(`𝄞`) is 4 in the Adobe point, 2 in a Kobo span and KOReader's CFI, and 1 in
KOReader's XPointer; a combining accent is one character everywhere.

**KOReader positions** (KOReader 2026.07, `cre_dom_version` 20260812):
`metadata.epub.lua` holds `last_xpointer` and also `last_epubcfi`. Spine
index *i* is `DocFragment[i+1]` and CFI step `/6/(2i+2)`, for kepub and plain
EPUB. At the start of the same chapter heading:

| Device | Stored | KOReader |
|---|---|---|
| Kobo, kepub | `xhtml-0-81.xhtml#kobo.1.1` | `/body[1]/DocFragment[82]/body[1]/div[1]/div[1]/div[1]/span[1]/text()[1].0` |
| Tolino, plain EPUB | `xhtml-0-81.xhtml#point(/1/2/1/1:0)` | `/body[1]/DocFragment[82]/body[1]/div[1]/text()[1].0` |

KOReader keeps the kepub wrappers `div#book-columns` / `div#book-inner` and
the `koboSpan` spans as elements, so a kepub and its source EPUB give
different XPointers for the same text. Element indices match the source DOM.
Positions taken by paging (end of Book C) are page starts and differ between
renderers; only chapter starts line up exactly.

**Store vs. sideloaded**: store books have UUID `ContentID`s on both
firmware lines; filtering on `file:///mnt/onboard/` is enough.

**Cross-device sample**: *Heat 2* is on both reference devices, but as two
different files (calibre kepub on the Kobo, a converted EPUB on the Tolino).
Both have 109 spine items and the same chapter hrefs, yet different hashes:
exactly the case the later manual mapping is for.

## Write-back over USB

`pull --write` (Tolino so far) changes only the book row
(`ContentType = 6`) of each pulled book, all books in one transaction,
after a byte-for-byte checked backup copy of the database on the desktop:

| Column | Set to |
|---|---|
| `ChapterIDBookmarked` | the server XPointer as an Adobe point (`href#point(/1/…:offset)`) |
| `___PercentRead` | the server percentage, rounded to whole percent |
| `DateLastRead` | unchanged, unless it is newer than the server position: then one second before the server's timestamp |

`DateLastRead`: the device has not been read since, so a fresh "now"
would be wrong, and it would make the device look newer than the server.
If the stored point then does not round-trip exactly (a chapter-only
fallback), the next `push` would send it back as a new position. Keeping
the date behind the server's timestamp keeps the server the newer side:
`push` leaves it alone and `pull` finds nothing to change. When `--force`
or the answer "server" takes the server position although the device was
read later, the date is moved to one second before the server's timestamp
for the same reason. Opening the book sets it again anyway.

The Tolino database is in **WAL mode** (file format bytes 18–19 are 2),
which `immutable=1` hides (`journal_mode` reports `delete` there). The
write keeps WAL mode, takes an exclusive lock first so SQLite needs no
`-shm` file on the FAT file system, and checkpoints after the commit so
the change is in `KoboReader.sqlite` itself; it refuses to start while a
`-journal`, `-wal` or `-shm` file exists and fails if one is left after.
Checked against a copy of the Shine Color database: integrity check
passes, no extra files, WAL mode kept.

Not changed: `CurrentChapterProgress` (position within the chapter, used
for the chapter estimate), `ReadStatus`, and `.kobo/BookReader.sqlite`,
which is encrypted. If Nickel ignores the new position, these are the
first suspects.

**Device test** (Shine Color 5.18, Book R, 2026-10-24, *verified*): KOReader
left Book R on a page starting near the end of R2-134 and synced
(`p[134]/text()[1].256`, 56 %). `pull --write` stored
`chapter2.xhtml#point(/1/4/270/1:269)` and 56 %; no `-wal`, `-shm` or
`-journal` file was left. After eject, Nickel opened Book R on the page
that holds that point (its top in R2-133) and, when the book was closed,
stored its own page start, `#point(/1/4/268/1:46)` (R2-133, 57 %), with
`DateLastRead` set to the time of opening and `CurrentChapterProgress`
updated. So Nickel takes the written position and does not overwrite it
with the old one; it snaps to the start of the page it renders, which can
lie up to a page before the written point. The database stays in WAL mode.

macOS left an AppleDouble file `._KoboReader.sqlite` next to the database
after the write; it was gone after Nickel had run, and Nickel did not
mind it.

## Open technical questions

1. **Shine 5 and Vision Color.** Assumed to match the Shine Color; not checked.
2. **Whitespace at the end of a block** and other cases the probe books do
   not cover (named entities from a DTD, `xml:space`, `<pre>`), and whether
   KOReader counts a whitespace-only text node in front of the target as a
   `text()` (the converter assumes it does). The offset rules in
   [Verified findings](#verified-findings) hold for everything Book P and
   Book Q test.
3. **Write-back.** Writing the position into the database only works safely
   while Nickel is not running (USB mode); see [Write-back over
   USB](#write-back-over-usb) for whether Nickel takes it. In the plugin
   the database is not enough (Nickel has read the book before the open
   hook runs and writes its own copy back); on 5.18 the open hook passes a
   copy of the book with the pulled position instead
   ([docs/nickel-5.18-symbols.md](docs/nickel-5.18-symbols.md)), still to
   be checked on the device. The Qt5 symbols for 4.46 are still open.

## Test data still needed

- Optional: a database from a Shine 5 or Vision Color
