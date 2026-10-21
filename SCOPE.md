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
reference Tolino (Shine 5) runs 5.18. The Rust core is unaffected; the
NickelHook plugin is built twice (Qt5 and Qt6).

## Book identity

KOSync identifies a book by a hash of its file. **First version: the user puts
the identical file on every device**, and NickelInkSync uses KOReader's default
"binary" hash (MD5 over 1 KiB samples at offsets 256, 1024, 4096, …, see
[Verified findings](#verified-findings)). A kepub on one device and the source
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
- **Conflict rule**: newest timestamp wins; never move a position backwards
  without user confirmation.

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
- Settings (server URL, user, device name) in `.adds/NickelInkSync/config.ini`.
- Packaging: `KoboRoot.tgz` for 4.x, `Kobo.tgz` for 5.x (hook in `imageformats/`).

## Verified findings

Reference devices, both read from a copy of the database, never written:

- **Kobo Clara BW**, firmware 4.46.23836, database version 176, two sideloaded kepubs
- **Tolino Shine 5**, firmware 5.18.264769, database version 220, two sideloaded plain EPUBs

| Question | Kobo 4.46 | Tolino 5.18 |
|---|---|---|
| Book row | `ContentType = 6`, `ContentID = file:///mnt/onboard/<path>` | same |
| MimeType | `application/x-kobo-epub+zip` (kepub) | `application/epub+zip` (plain EPUB) |
| Chapter rows | `ContentType = 9`, one per spine item, `ContentID = /mnt/onboard/<path>!<opf dir>!<href>`; plus `ContentType = 899`, one per ToC entry | `ContentType = 9`, one per **ToC entry** (no spine rows), `ContentID` is a **UUID**, target in `adobe_location` |
| Chapter → book | `BookID` = book `ContentID`, `VolumeIndex` = spine index (type 9) | `BookID` = book `ContentID`, `VolumeIndex` = **ToC index**, not spine index |
| Position field | `ChapterIDBookmarked`, `___PercentRead` (integer percent) | same |
| Position format | `<href>#kobo.N.M` (kepub span ID) | `<href>#point(/1/2/1/1:0)` (Adobe point) |
| `adobe_location` | empty | empty, even for plain EPUB |
| Extra tables | — | `tolino_*` (cloud, DRM, Skoobe); `BookReader.sqlite` is encrypted and not needed |

In the chapter-row `ContentID`, `<href>` is relative to the OPF directory;
the chapter path in `ChapterIDBookmarked` and `adobe_location` is relative to
the EPUB root (`OEBPS/xhtml/…`). Never rely on the chapter-row `ContentID`
format: it differs between firmware lines. On the Kobo, `BookID` +
`VolumeIndex` of the `ContentType = 9` rows gives the spine item. On the
Tolino it does not: the rows are ToC entries (`NumShortcovers` counts them),
so in *Heat 2* `xhtml-0-81.xhtml` (spine index 81) has `VolumeIndex` 80, and
the *Titan 2* position lies in a spine item without a ToC entry and so
without a row. Find the chapter through the OPF spine there. Both cases are
in the [test fixtures](tests/fixtures/README.md).

**Kepub span → XPointer** (Kobo): `OEBPS/xhtml/xhtml-0-81.xhtml#kobo.1.172`
→ spine index 81 → KOReader `DocFragment[82]`; span `id="kobo.1.172"` exists
in the chapter file. Resolve the span by its ID; don't derive
paragraph/sentence from `N.M` (calibre's kepub output numbers differently from
kepubify).

**Adobe point → DOM node** (Tolino): `point(/a/b/c…:offset)` is a path of
1-based indices over **all** child nodes, whitespace text nodes included,
where `/1` is the `html` element; `offset` is a character offset into the
final text node. Checked on two books: `/1/2/1/1:0` hits the chapter heading
in *Heat 2*, `/1/4/16/1:356` hits mid-paragraph text in *Star Trek Titan 2*.
Two samples, so the edge cases (entities, CDATA, comments) are still open.

**Store vs. sideloaded**: store books have UUID `ContentID`s on both
firmware lines; filtering on `file:///mnt/onboard/` is enough.

**Cross-device sample**: *Heat 2* is on both reference devices, but as two
different files (calibre kepub on the Kobo, a converted EPUB on the Tolino).
Both have 109 spine items and the same chapter hrefs, yet different hashes:
exactly the case the later manual mapping is for.

## Open technical questions

1. **Plain EPUB on Kobo 4.x.** Only checked on Tolino 5.18. Kobo 4.x may still
   use `adobe_location` for plain EPUBs.
2. **Shine Color and Vision Color.** Assumed to match the Shine 5; not checked.
3. **XPointer below chapter level.** KOReader addresses a text node by DOM
   path (`/body/DocFragment[82]/body/div/p[12]/text().0`). Mapping a Kobo span
   or an Adobe point to that path means replicating how KOReader's crengine
   builds its DOM (whitespace handling, whether it keeps `koboSpan` elements),
   checked against a KOReader dump.
4. **Write-back.** Writing the position into the database only works safely
   while Nickel is not running (USB mode). The plugin path needs a libnickel
   symbol to jump to a position, on both Qt5 and Qt6 builds.

## Test data still needed

- The KOReader state for one of the reference books
  (`<book>.sdr/metadata.epub.lua`), to check the XPointer format below chapter level
- A sideloaded plain EPUB on the Kobo, opened at a known spot
- Optional: a database from a Shine Color or Vision Color
