# Test fixtures

Trimmed, anonymized copies of real device data from the two reference
devices, and small placeholder EPUBs with the structure of the real books.
The device databases themselves and the books never enter the repository
(the CI rejects `*.sqlite`), only what the scripts in [`tools/`](../../tools)
derive from them.

## Device databases (`db/`)

| Fixture | Device | Firmware | DbVersion | Books |
|---|---|---|---|---|
| [`kobo-clarabw-4.46.sql`](db/kobo-clarabw-4.46.sql) | Kobo Clara BW | 4.46.23836 | 176 | Book A, Book B (kepub) |
| [`tolino-shine5-5.18.sql`](db/tolino-shine5-5.18.sql) | Tolino Shine 5 | 5.18.264769 | 220 | Book A, Book C (plain EPUB) |

Book A is the same title on both devices (as two different files: a kepub on
the Kobo, an EPUB on the Tolino), so cross-device tests can use it.

Each file is SQL text: the schema, then one `INSERT` per row, sorted by
book, row type, `VolumeIndex` and `ContentID`. Load it with
`Connection::execute_batch` (see [`fixtures_db.rs`](../fixtures_db.rs)) or
`sqlite3 :memory: '.read <file>'`.

What is in it:

- the full `content` schema with its indexes, and `DbVersion`;
- the rows of the sideloaded books (`ContentType = 6`, `ContentID` under
  `file:///mnt/onboard/`) and of their chapters (`ContentType` 9 and 899);
- on the Tolino, the **empty** schemas of `tolino_bookmark`,
  `tolino_contentItem` and `tolino_dogear`, for the Tolino detection.

What is not: store books, Instapaper articles, `Bookmark`, `user`, `Event`,
`Activity`, `AnalyticsEvents` or any other table, and no row of any
`tolino_*` table.

Anonymized:

| Value | In the fixture |
|---|---|
| Title, `BookTitle` | `Book A`, `Book B`, `Book C` |
| `Attribution` (authors) | `Author A`, … |
| File name in `ContentID`, `BookID`, `ImageId` and in chapter `ContentID` / `ChapterIDBookmarked` (`<path>!<opf dir>!<href>`) | `book-a.kepub.epub`, `book-a.epub`, …; directory and extension kept |
| Table-of-contents titles (Kobo `ContentType` 899, Tolino chapter rows) | `Entry <VolumeIndex>` |
| Tolino chapter `ContentID` (UUID) | new random version-4 UUIDs, fixed seed |
| calibre book UUID in Kobo ToC anchors | `000…001`, `000…002`, … |
| Description, publisher, ISBN, series, subtitle, `metaDataJson`, `imageMetaDataJson`, store and account IDs | `NULL` |
| `DateLastRead` | `2026-01-01T12:00:00Z`, `2026-01-02T12:00:00Z`, … in the order the books were last read on each device |
| Other timestamps (`___SyncTime`, `DateCreated`, …) | `2026-01-01T00:00:00Z` |
| `___UserID` | kept only for Nickel's generic values (`kepub_user`, `adobe_user`) |

Kept unchanged: `ChapterIDBookmarked` of the book rows (chapter path plus
`#kobo.N.M` or `#point(...)`), `___PercentRead`, `ReadStatus`,
`VolumeIndex`, `MimeType`, `ContentType`, `adobe_location` (chapter targets),
`NumShortcovers`, file sizes and the other numeric and flag columns. Any
other text value stops the script, so new kinds of data are never copied
silently.

Before writing, the script checks the output for leftovers of the original:
titles, authors, words from them, file names, paths, publisher, ISBN, the
original UUIDs, every value of the device's `user` table, and serial numbers
(`T30…`). It also loads the SQL back and compares every value, type
included.

## Books (`epub/`)

| Fixture | Stands for | Kept chapter | Stored position |
|---|---|---|---|
| [`book-a.kepub.epub`](epub/book-a.kepub.epub) | Book A on the Kobo | `OEBPS/xhtml/xhtml-0-81.xhtml` | `#kobo.1.172` |
| [`book-a.epub`](epub/book-a.epub) | Book A on the Tolino | `OEBPS/xhtml/xhtml-0-81.xhtml` | `#point(/1/2/1/1:0)` |
| [`book-c.epub`](epub/book-c.epub) | Book C on the Tolino | `OPS/bookwire/bookwire_advertisement5.xhtml` | `#point(/1/4/16/1:356)` |

Each has the OPF of the original book (same directory, same spine: item
count, ids, hrefs and order), its NCX and nav document with the same nesting
and targets, and the chapter with the stored position as a skeleton: every
element with its `id` and `class`, every whitespace text node unchanged,
every other character replaced by `x`. Node paths and character offsets
therefore resolve exactly as in the original; the script checks this for
each stored position. The other spine items are one-line placeholder pages;
images, fonts and style sheets are left out.

Changed on purpose: digit runs of ten or more (ISBNs, a distributor ID) are
zeroed in file names and ids, so Book C's OPF is `OPS/0000000000000.opf`
and its spine ids are `part_N_000000000000000000`. Links to the outside
become `#`.

## Regenerating

The scripts need the original files, which stay on the owner's machine:
copies of `.kobo/KoboReader.sqlite` from both devices and the three books.
They only read them (the databases as an immutable SQLite URI).

```sh
tools/make_db_fixture.py clarabw-KoboReader.sqlite tests/fixtures/db/kobo-clarabw-4.46.sql \
  --names a,b --device "Kobo Clara BW, firmware 4.46.23836"
tools/make_db_fixture.py shine5-KoboReader.sqlite tests/fixtures/db/tolino-shine5-5.18.sql \
  --names a,c --device "Tolino Shine 5, firmware 5.18.264769"
tools/make_epub_fixture.py <book a, kepub> tests/fixtures/epub/book-a.kepub.epub --name a \
  --keep OEBPS/xhtml/xhtml-0-81.xhtml --position 'OEBPS/xhtml/xhtml-0-81.xhtml#kobo.1.172'
tools/make_epub_fixture.py <book a, epub> tests/fixtures/epub/book-a.epub --name a \
  --keep OEBPS/xhtml/xhtml-0-81.xhtml --position 'OEBPS/xhtml/xhtml-0-81.xhtml#point(/1/2/1/1:0)'
tools/make_epub_fixture.py <book c> tests/fixtures/epub/book-c.epub --name c \
  --keep OPS/bookwire/bookwire_advertisement5.xhtml \
  --position 'OPS/bookwire/bookwire_advertisement5.xhtml#point(/1/4/16/1:356)'
```

`--names` assigns the letters in the order of the original `ContentID`s. The
output is deterministic: the same input gives byte-identical files.

The scripts are Python (standard library only) rather than Rust examples:
they run once on the owner's machine, need SQLite, ZIP and XML handling that
the crate itself does not need yet, and Python is already set up in the CI.
