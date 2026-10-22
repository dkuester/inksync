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
| [`tolino-shinecolor-5.18.sql`](db/tolino-shinecolor-5.18.sql) | Tolino Shine Color | 5.18.264769 | 220 | Book A, Book C (plain EPUB) |
| [`kobo-clarabw-4.46-placeholder-books.sql`](db/kobo-clarabw-4.46-placeholder-books.sql) | Kobo Clara BW | 4.46.23836 | 176 | the three EPUB fixtures below |
| [`kobo-clarabw-4.46-probe-book.sql`](db/kobo-clarabw-4.46-probe-book.sql) | Kobo Clara BW | 4.46.23836 | 176 | Book P (kepub and EPUB) with highlights |
| [`tolino-shinecolor-5.18-probe-book.sql`](db/tolino-shinecolor-5.18-probe-book.sql) | Tolino Shine Color | 5.18.264769 | 220 | Book P (EPUB) with highlights |
| [`tolino-shinecolor-5.18-placeholder-books.sql`](db/tolino-shinecolor-5.18-placeholder-books.sql) | Tolino Shine Color | 5.18.264769 | 220 | the three EPUB fixtures below |

Book A is the same title on both devices (as two different files: a kepub on
the Kobo, an EPUB on the Tolino), so cross-device tests can use it.

The `-placeholder-books` fixtures come from the same devices after they were
emptied and loaded with the files from [`epub/`](epub) (byte-identical), then
opened at known places. Their positions match the files in the repository and
the KOReader state in [`koreader/`](#koreader-state-koreader); see the table
there.

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

## Probe book (`epub/book-p*.epub`)

Book P is synthetic, made by [`make_probe_epub.py`](../../tools/make_probe_epub.py)
without any input, as a plain EPUB ([`book-p.epub`](epub/book-p.epub)) and a
kepub with Kobo spans ([`book-p.kepub.epub`](epub/book-p.kepub.epub)). Every
word is unique (`p<paragraph>w<word>`), so the same word can be highlighted
on every device. The probe chapter `OEBPS/text/probe.xhtml` (spine index 1)
puts one of the cases that can shift character offsets in front of each
target word.

**To test:** put both files on the Kobo, `book-p.epub` on the Tolino, and both
into KOReader. In each, highlight the eight target words below, one
highlight per word, starting exactly at the word. Then copy
`.kobo/KoboReader.sqlite` from both devices and the `.sdr` folders from
KOReader. `make_db_fixture.py --bookmarks` keeps the highlights of the fixture
books; highlights in any other book stop it.

Where each format should put the start of the word, as predicted by
`make_probe_epub.py` from the source DOM (the results follow below):

| Word | After | EPUB: Adobe point | kepub: span, offset | KOReader EPUB (naive) | KOReader kepub (naive) |
|---|---|---|---|---|---|
| `p1w08` | plain text, mid-paragraph | `point(/1/4/4/1:42)` | `kobo.2.1`, 42 | `/body[1]/DocFragment[2]/body[1]/p[1]/text()[1].42` | `/body[1]/DocFragment[2]/body[1]/div[1]/div[1]/p[1]/span[1]/text()[1].42` |
| `p2w04` | two no-break spaces | `point(/1/4/6/1:18)` | `kobo.3.1`, 18 | `/body[1]/DocFragment[2]/body[1]/p[2]/text()[1].18` | `/body[1]/DocFragment[2]/body[1]/div[1]/div[1]/p[2]/span[1]/text()[1].18` |
| `p3w03` | the entity `&amp;` | `point(/1/4/8/1:14)` | `kobo.4.1`, 14 | `/body[1]/DocFragment[2]/body[1]/p[3]/text()[1].14` | `/body[1]/DocFragment[2]/body[1]/div[1]/div[1]/p[3]/span[1]/text()[1].14` |
| `p4w03` | a word with a soft hyphen | `point(/1/4/10/1:13)` | `kobo.5.1`, 13 | `/body[1]/DocFragment[2]/body[1]/p[4]/text()[1].13` | `/body[1]/DocFragment[2]/body[1]/div[1]/div[1]/p[4]/span[1]/text()[1].13` |
| `p5w03` | inside `<em>`, second word | `point(/1/4/12/2/1:6)` | `kobo.6.2`, 6 | `/body[1]/DocFragment[2]/body[1]/p[5]/em[1]/text()[1].6` | `/body[1]/DocFragment[2]/body[1]/div[1]/div[1]/p[5]/em[1]/span[1]/text()[1].6` |
| `p5w04` | text after `</em>` | `point(/1/4/12/3:1)` | `kobo.6.3`, 1 | `/body[1]/DocFragment[2]/body[1]/p[5]/text()[2].1` | `/body[1]/DocFragment[2]/body[1]/div[1]/div[1]/p[5]/span[2]/text()[1].1` |
| `p6w03` | a line break and indentation | `point(/1/4/14/1:16)` | `kobo.7.1`, 16 | `/body[1]/DocFragment[2]/body[1]/p[6]/text()[1].16` | `/body[1]/DocFragment[2]/body[1]/div[1]/div[1]/p[6]/span[1]/text()[1].16` |
| `p7w03` | a comment | `point(/1/4/16/3:7)` | `kobo.8.2`, 7 | `/body[1]/DocFragment[2]/body[1]/p[7]/text()[2].7` | `/body[1]/DocFragment[2]/body[1]/div[1]/div[1]/p[7]/span[2]/text()[1].7` |

**Results** (2026-10-22): the eight words highlighted on the Kobo (kepub and
EPUB), the Tolino (EPUB) and in KOReader (both); stored in the
`-probe-book` SQL fixtures (`Bookmark` rows) and in
`koreader/book-p*.sdr/`. Bold where the prediction was wrong:

| Word | Kobo kepub | Kobo EPUB | Tolino EPUB | KOReader EPUB | KOReader kepub |
|---|---|---|---|---|---|
| `p1w08` | `kobo.2.1`, 42 | `/1/4/4/1:42` | `/1/4/4/1:42` | `p[1]/text()[1].42` | `…/span[1]/text()[1].42` |
| `p2w04` | `kobo.3.1`, 18 | **`/1/4/6/1:20`** | **`/1/4/6/1:20`** | `p[2]/text()[1].18` | `….18` |
| `p3w03` | `kobo.4.1`, 14 | `/1/4/8/1:14` | `/1/4/8/1:14` | `p[3]/text()[1].14` | `….14` |
| `p4w03` | `kobo.5.1`, 13 | **`/1/4/10/1:14`** | **`/1/4/10/1:14`** | `p[4]/text()[1].13` | `….13` |
| `p5w03` | `kobo.6.2`, 6 | `/1/4/12/2/1:6` | `/1/4/12/2/1:6` | `p[5]/em[1]/text()[1].6` | `…/em[1]/span[1]/text()[1].6` |
| `p5w04` | `kobo.6.3`, 1 | `/1/4/12/3:1` | `/1/4/12/3:1` | `p[5]/text()[2].1` | `…/span[2]/text()[1].1` |
| `p6w03` | `kobo.7.1`, 16 | `/1/4/14/1:16` ¹ | `/1/4/14/1:16` | **`p[6]/text()[1].12`** | **`….12`** |
| `p7w03` | `kobo.8.2`, 7 | **`/1/4/16/1:13`** | `/1/4/16/3:7` | `p[7]/text()[2].7` | `…/span[2]/text()[1].7` |

¹ The selection started at the space before the word (`/1/4/14/1:15`).

Highlighting on the Kobo 4.46 is fiddly, with two display bugs while the
stored rows are correct: in the kepub some highlights were shown a few
characters off, in the plain EPUB some stayed invisible until the font size
was changed a few times. Trust the database, not the screen. The Tolino
5.18 showed neither.

- Adobe point offsets count **UTF-8 bytes**: no-break space and soft hyphen
  are two bytes each (p2w04, p4w03). Whitespace is counted as is (p6w03).
- Kobo 4.46 skips the comment and joins the text around it (`/1:13`), the
  Tolino 5.18 counts the comment as a node (`/3:7`).
- Kobo span offsets count characters, whitespace as is.
- KOReader counts characters after collapsing whitespace (p6w03: 12) and
  starts a new `text()` after a comment; its CFI joins the text instead
  (`epubcfi(/6/4!/4/16,/1:13,…)`).

## Second probe book (`epub/book-q*.epub`)

Book Q is made by the same script (`make_probe_epub.py --book q`), with its
own name and `q…` words so it cannot be confused with Book P. Its probe
chapter covers what Book P left open: umlauts and `ß`, three-byte characters
(`–`, `…`, `„`, `“`), a four-byte character (`𝄞`, two UTF-16 units), CDATA,
leading and repeated spaces, `&lt;`/`&gt;` with a tab, and a combining
accent (`Cafe` + U+0301). The device may draw `𝄞` as a box; that does not
matter, only the word after it is highlighted.

**To test:** as for Book P, with the eight words below, on the Kobo
(`book-q.kepub.epub` and `book-q.epub`), the Tolino (`book-q.epub`) and in
KOReader (both).

Predicted with the rules found with Book P (Adobe point in UTF-8 bytes,
Kobo span in characters, KOReader in characters after collapsing whitespace
runs); where Book Q breaks a rule, the highlights show it:

| Word | After | EPUB: Adobe point | kepub: span, offset | KOReader EPUB | KOReader kepub |
|---|---|---|---|---|---|
| `q1w03` | umlauts and ß (two bytes each) | `point(/1/4/4/1:27)` | `kobo.2.1`, 22 | `/body[1]/DocFragment[2]/body[1]/p[1]/text()[1].22` | `/body[1]/DocFragment[2]/body[1]/div[1]/div[1]/p[1]/span[1]/text()[1].22` |
| `q2w03` | `–` and `…` (three bytes each) | `point(/1/4/6/1:20)` | `kobo.3.1`, 16 | `/body[1]/DocFragment[2]/body[1]/p[2]/text()[1].16` | `/body[1]/DocFragment[2]/body[1]/div[1]/div[1]/p[2]/span[1]/text()[1].16` |
| `q3w03` | typographic quotes `„` `“` (three bytes each) | `point(/1/4/8/1:18)` | `kobo.4.1`, 14 | `/body[1]/DocFragment[2]/body[1]/p[3]/text()[1].14` | `/body[1]/DocFragment[2]/body[1]/div[1]/div[1]/p[3]/span[1]/text()[1].14` |
| `q4w02` | a four-byte character (`𝄞`, two UTF-16 units) | `point(/1/4/10/1:11)` | `kobo.5.1`, 8 | `/body[1]/DocFragment[2]/body[1]/p[4]/text()[1].8` | `/body[1]/DocFragment[2]/body[1]/div[1]/div[1]/p[4]/span[1]/text()[1].8` |
| `q5w04` | a CDATA section | `point(/1/4/12/3:1)` | `kobo.6.2`, 1 | `/body[1]/DocFragment[2]/body[1]/p[5]/text()[2].1` | `/body[1]/DocFragment[2]/body[1]/div[1]/div[1]/p[5]/span[2]/text()[1].1` |
| `q6w03` | leading and repeated spaces | `point(/1/4/14/1:18)` | `kobo.7.1`, 18 | `/body[1]/DocFragment[2]/body[1]/p[6]/text()[1].13` | `/body[1]/DocFragment[2]/body[1]/div[1]/div[1]/p[6]/span[1]/text()[1].13` |
| `q7w03` | `&lt;`, `&gt;` and a tab | `point(/1/4/16/1:14)` | `kobo.8.1`, 14 | `/body[1]/DocFragment[2]/body[1]/p[7]/text()[1].14` | `/body[1]/DocFragment[2]/body[1]/div[1]/div[1]/p[7]/span[1]/text()[1].14` |
| `q8w02` | `e` with a combining accent | `point(/1/4/18/1:13)` | `kobo.9.1`, 12 | `/body[1]/DocFragment[2]/body[1]/p[8]/text()[1].12` | `/body[1]/DocFragment[2]/body[1]/div[1]/div[1]/p[8]/span[1]/text()[1].12` |

## KOReader state (`koreader/`)

`metadata.epub.lua` from KOReader 2026.07 on macOS (`cre_dom_version`
20260812), one per EPUB fixture, opened from the identical files. Only
`doc_path` was changed (`/home/reader/books/…`); the files hold no other
personal data. Also `book-p.sdr` and `book-p.kepub.sdr` with the Book P highlights
(`annotations`, each with `pos0`/`pos1` and `epubcfi`). Each has the KOReader
partial MD5 of the file
(`partial_md5_checksum`) and the position as XPointer and EPUB CFI.

| File | Kobo 4.46 | Tolino 5.18 | KOReader `last_xpointer` |
|---|---|---|---|
| `book-a.kepub.epub` | `xhtml-0-81.xhtml#kobo.1.1` | `xhtml-0-7.xhtml#point(/1/2/1/1:0)` | `/body[1]/DocFragment[82]/body[1]/div[1]/div[1]/div[1]/span[1]/text()[1].0` |
| `book-a.epub` | `xhtml-0-7.xhtml` (no fragment) | `xhtml-0-81.xhtml#point(/1/2/1/1:0)` | `/body[1]/DocFragment[82]/body[1]/div[1]/text()[1].0` |
| `book-c.epub` | `front02.html#point(/1/2/1/1:0)` | `bookwire_advertisement5.xhtml#point(/1/4/16/1:688)` | `/body[1]/DocFragment[40]/body[1]/div[2]/text()[1].0` |

Same place in two of them:

- Kobo `#kobo.1.1` ↔ KOReader on the kepub: `span#kobo.1.1`, the start of the
  chapter heading in `xhtml-0-81.xhtml`. KOReader keeps the kepub wrappers
  (`div#book-columns`, `div#book-inner`) and the `koboSpan` spans.
- Tolino `point(/1/2/1/1:0)` ↔ KOReader on the plain EPUB: the same heading,
  `div#aid_191`, offset 0.
- Book C on Tolino and KOReader: both on the last page, but each stores the
  start of its own page, so the nodes differ (the teaser paragraph at
  character 688 vs. the following `div`).

## Regenerating

The scripts need the original files, which stay on the owner's machine:
copies of `.kobo/KoboReader.sqlite` from both devices and the three books.
They only read them (the databases as an immutable SQLite URI).

```sh
tools/make_db_fixture.py clarabw-KoboReader.sqlite tests/fixtures/db/kobo-clarabw-4.46.sql \
  --names a,b --device "Kobo Clara BW, firmware 4.46.23836"
tools/make_db_fixture.py shinecolor-KoboReader.sqlite tests/fixtures/db/tolino-shinecolor-5.18.sql \
  --names a,c --device "Tolino Shine Color, firmware 5.18.264769"
tools/make_epub_fixture.py <book a, kepub> tests/fixtures/epub/book-a.kepub.epub --name a \
  --keep OEBPS/xhtml/xhtml-0-81.xhtml --position 'OEBPS/xhtml/xhtml-0-81.xhtml#kobo.1.172'
tools/make_epub_fixture.py <book a, epub> tests/fixtures/epub/book-a.epub --name a \
  --keep OEBPS/xhtml/xhtml-0-81.xhtml --position 'OEBPS/xhtml/xhtml-0-81.xhtml#point(/1/2/1/1:0)'
tools/make_epub_fixture.py <book c> tests/fixtures/epub/book-c.epub --name c \
  --keep OPS/bookwire/bookwire_advertisement5.xhtml \
  --position 'OPS/bookwire/bookwire_advertisement5.xhtml#point(/1/4/16/1:356)'
```

The `-placeholder-books` fixtures are made the same way from the later
database copies, with `--names a,a,c`; the KOReader files are copied from
`<book>.sdr/` with `doc_path` replaced.

`--names` assigns the letters in the order of the original `ContentID`s. The
output is deterministic: the same input gives byte-identical files.

The scripts are Python (standard library only) rather than Rust examples:
they run once on the owner's machine, need SQLite, ZIP and XML handling that
the crate itself does not need yet, and Python is already set up in the CI.
