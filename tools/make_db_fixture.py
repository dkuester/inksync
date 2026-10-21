#!/usr/bin/env python3
"""Turn a copy of a device `KoboReader.sqlite` into an anonymized SQL fixture.

Keeps only what the database reader (K1) needs: the full schema of `content`
and `DbVersion`, the rows of the sideloaded books (`ContentType = 6`,
`ContentID` under `file:///mnt/onboard/`) and of their chapters (`ContentType`
9 and 899), and, on a Tolino, the empty schemas of a few `tolino_*` tables so
the Tolino detection can be tested. Nothing else is read into the output:
no store books, no `Bookmark`, `user`, `Event` or analytics rows, and never
the contents of a `tolino_*` table (DRM keys live there). With `--bookmarks`
it also keeps the `Bookmark` rows (highlights) of the sideloaded books, but
only if their text consists of probe words (`p1w08`) or `x` placeholders,
so highlights in a real book stop the script.

Titles, authors, file names and every other descriptive text are replaced by
placeholders; the structural values (positions, percent read, spine and ToC
indices, MIME and content types) are kept. At the end the output is checked
for leftovers of the original data, and the script fails if it finds any.

    tools/make_db_fixture.py SOURCE.sqlite OUTPUT.sql --names a,b --device "..."

`--names` gives the placeholder letter of each book, in the order of the
original `ContentID`s, so the same book can carry the same letter on several
devices. The source is opened as an immutable URI and never written.
"""

import argparse
import random
import re
import sqlite3
import sys
import uuid
from pathlib import PurePosixPath

SIDELOADED = "file:///mnt/onboard/"
# Empty schemas only, for the Tolino detection. Never their rows.
TOLINO_TABLES = ["tolino_bookmark", "tolino_contentItem", "tolino_dogear"]

# Descriptive or account-related columns: always NULL in the fixture.
NULLED = {
    "Description", "Publisher", "ISBN", "Series", "SeriesNumber", "SeriesNumberFloat",
    "SeriesID", "Subtitle", "metaDataJson", "imageMetaDataJson", "ContentURL",
    "ImageUrl", "WorkId", "PublicationId", "EntitlementId", "CrossRevisionId",
    "ExternalId", "ExternalIds", "PurchaseRevisionId", "AnnotationsSyncToken",
    "ShortCoverKey", "BookshelfTags", "titleKana", "subtitleKana", "seriesKana",
    "attributionKana", "publisherKana", "familySharingRelation",
    "ApplicableSubscriptions", "UnsyncedPocketChanges", "Properties", "Fallback",
}
# Timestamps other than DateLastRead: set to one fixed value.
TIMESTAMPS = {
    "___SyncTime", "DateCreated", "DateAdded", "DateModified", "WishlistedDate",
    "ReviewsSyncDate", "LastTimeStartedReading", "LastTimeFinishedReading",
    "AdobeLoanExpiration",
}
FIXED_TIME = "2026-01-01T00:00:00Z"
# Placeholder values Nickel itself writes; anything else in a column without
# an explicit rule stops the script, so new kinds of data are never copied
# silently.
SAFE_TEXT = re.compile(
    r"^(|true|false|TRUE|FALSE|default|-?\d+(\.\d+)?|[a-z]{2}([-_][A-Za-z]{2})?"
    r"|kepub_user|adobe_user|0000-00-00T00:00:00\.000)$"
)
HEX32 = re.compile(r"[0-9a-f]{32}")
UUID = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
SERIAL = re.compile(r"T30[0-9A-Z]{4,}")
# Highlighted text allowed in a fixture: probe words, x placeholders, spacing
# and punctuation.
HIGHLIGHT_TEXT = re.compile(r"^(?:p\d+w\d+|x+|[\s.,;:&\u00ad-])*$")


def sql_literal(value):
    if value is None:
        return "NULL"
    if isinstance(value, (int, float)):
        return repr(value)
    return "'" + value.replace("'", "''") + "'"


class Anonymizer:
    def __init__(self, books, names, seed):
        if len(names) != len(books):
            sys.exit(f"--names has {len(names)} entries, the database has {len(books)} sideloaded books")
        self.rng = random.Random(seed)
        self.paths = {}  # original device path -> placeholder path
        self.letters = {}  # original book ContentID -> letter
        self.uuids = {}
        self.hex_ids = {}
        for book, letter in zip(books, names):
            old = book["ContentID"][len("file://"):]
            path = PurePosixPath(old)
            suffix = ".kepub.epub" if path.name.endswith(".kepub.epub") else path.suffix
            self.paths[old] = str(path.parent / f"book-{letter}{suffix}")
            self.letters[book["ContentID"]] = letter
        # Longest first, so a path that contains another one is replaced whole.
        self.order = sorted(self.paths, key=len, reverse=True)

    def path_text(self, text):
        for old in self.order:
            text = text.replace(old, self.paths[old])
        return HEX32.sub(lambda m: self.hex_ids.setdefault(m[0], f"{len(self.hex_ids) + 1:032x}"), text)

    def chapter_uuid(self, value):
        if value not in self.uuids:
            self.uuids[value] = str(uuid.UUID(int=self.rng.getrandbits(128), version=4))
        return self.uuids[value]


def image_id_transform(content_id, image_id):
    """Nickel derives `ImageId` from the `ContentID`; find which characters it
    replaced on this firmware so the placeholder path gets the same treatment."""
    for chars in ("/: .", "/: "):
        derived = re.sub(f"[{re.escape(chars)}]", "_", content_id)
        if derived == image_id:
            return lambda text, chars=chars: re.sub(f"[{re.escape(chars)}]", "_", text)
    sys.exit(f"ImageId does not follow a known rule: {image_id!r}")


def anonymize_row(row, columns, anon, book_letter, last_read):
    out = {}
    is_book = row["ContentType"] == "6"
    for col in columns:
        value = row[col]
        if value is None or isinstance(value, (int, float)):
            out[col] = value
        elif col in ("ContentType", "MimeType"):
            out[col] = value
        elif col in ("ContentID", "BookID", "ChapterIDBookmarked"):
            if col == "ContentID" and UUID.match(value):
                out[col] = anon.chapter_uuid(value)
            else:
                out[col] = anon.path_text(value)
        elif col == "ImageId" and value:
            out[col] = image_id_transform(row["ContentID"], value)(anon.path_text(row["ContentID"]))
        elif col == "BookTitle":
            out[col] = f"Book {book_letter.upper()}"
        elif col == "Title":
            href = row["ContentID"].rsplit("!", 1)[-1] if "!" in row["ContentID"] else None
            if is_book:
                out[col] = f"Book {book_letter.upper()}"
            elif value == href:
                out[col] = value  # Kobo spine rows: the title is the chapter href.
            else:
                out[col] = f"Entry {row['VolumeIndex']}"
        elif col == "Attribution":
            out[col] = f"Author {book_letter.upper()}"
        elif col == "adobe_location":
            out[col] = anon.path_text(value)  # Chapter href within the EPUB.
        elif col == "DateLastRead":
            out[col] = last_read[row["ContentID"]] if is_book else FIXED_TIME
        elif col in NULLED:
            out[col] = None
        elif col in TIMESTAMPS:
            out[col] = value if value.startswith("0000-") else FIXED_TIME
        elif col == "___UserID":
            out[col] = value if SAFE_TEXT.match(value) else "user"
        elif SAFE_TEXT.match(value):
            out[col] = value
        else:
            sys.exit(f"No rule for column {col} with value {value[:40]!r}…; extend the script")
    return out


def forbidden_strings(db, books, chapters):
    """Original values that must not survive in the output."""
    strings, tokens = set(), set()
    for row in books:
        path = row["ContentID"][len("file://"):]
        name = PurePosixPath(path).name
        strings |= {path, name, name.split(".")[0]}
        for col in ("Title", "Attribution", "Publisher", "ISBN", "Series", "Subtitle", "Description"):
            if row[col]:
                strings.add(row[col])
                tokens |= set(re.findall(r"\w{5,}", row[col] if col != "Description" else ""))
        for author in re.split(r",|&| and ", row["Attribution"] or ""):
            strings.add(author.strip())
    for row in chapters:
        if UUID.match(row["ContentID"]):
            strings.add(row["ContentID"])
        strings |= set(HEX32.findall(row["ContentID"]))
    user_cols = [r[1] for r in db.execute("PRAGMA table_info(user)")]
    for row in db.execute("SELECT * FROM user"):
        for col, value in zip(user_cols, row):
            if isinstance(value, str) and len(value) >= 6 and not SAFE_TEXT.match(value):
                strings.add(value)
    return {s for s in strings if s and len(s) >= 4}, tokens


def check_leftovers(text, schema, strings, tokens):
    data = text[len(schema):]
    problems = [f"original value ({len(s)} chars, starts {s[:3]!r}…)" for s in strings if s in data]
    schema_words = set(re.findall(r"\w+", schema.lower()))
    problems += [f"title or author word {t[:3]!r}…" for t in tokens
                 if t.lower() not in schema_words and re.search(rf"\b{re.escape(t)}\b", data, re.I)]
    if SERIAL.search(data):
        problems.append("device serial number (T30…)")
    if problems:
        sys.exit("Leftovers of the original data in the output, nothing written:\n  " + "\n  ".join(problems))


def anonymize_bookmark(row, columns, anon):
    out = {}
    for col in columns:
        value = row[col]
        if value is None or isinstance(value, (int, float)):
            out[col] = value
        elif isinstance(value, bytes) or col in ("Annotation", "markupJson", "UserID", "Creator"):
            out[col] = None  # Notes, drawings and account data.
        elif col in ("BookmarkID", "UUID"):
            out[col] = anon.chapter_uuid(value)
        elif col == "VolumeID":
            out[col] = anon.path_text(value)
        elif col == "ContentID":
            out[col] = anon.chapter_uuid(value) if UUID.match(value) else anon.path_text(value)
        elif col in ("StartContainerPath", "EndContainerPath"):
            out[col] = value
        elif col in ("Text", "ContextString"):
            if not HIGHLIGHT_TEXT.match(value):
                sys.exit(f"Bookmark {col} is not probe or placeholder text; only highlights in the fixture books are allowed")
            out[col] = value
        elif col in ("DateCreated", "DateModified", "SyncTime"):
            out[col] = FIXED_TIME
        elif col in ("Type", "Version") or SAFE_TEXT.match(value):
            out[col] = value
        else:
            sys.exit(f"No rule for Bookmark column {col} with value {value[:40]!r}…; extend the script")
    return out


def check_round_trip(text, expected, columns, table="content"):
    """Load the fixture and compare every value, type included."""
    mem = sqlite3.connect(":memory:")
    mem.executescript(text)
    loaded = mem.execute(f"SELECT {', '.join(columns)} FROM {table} ORDER BY rowid").fetchall()
    want = [tuple(row[c] for c in columns) for row in expected]
    typed = lambda rows: [tuple((type(v).__name__, v) for v in row) for row in rows]  # noqa: E731
    if typed(loaded) != typed(want):
        sys.exit("The SQL fixture does not load back to the anonymized rows")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("source", help="copy of KoboReader.sqlite (opened immutable, never written)")
    parser.add_argument("output", help="SQL fixture to write")
    parser.add_argument("--names", required=True, help="placeholder letters in ContentID order, e.g. a,b")
    parser.add_argument("--device", required=True, help="device and firmware, for the header comment")
    parser.add_argument("--bookmarks", action="store_true", help="also keep the highlights of the sideloaded books")
    args = parser.parse_args()

    db = sqlite3.connect(f"file:{args.source}?immutable=1", uri=True)
    db.row_factory = sqlite3.Row
    books = db.execute(
        "SELECT * FROM content WHERE ContentType = '6' AND ContentID LIKE ? ORDER BY ContentID",
        (SIDELOADED + "%",),
    ).fetchall()
    ids = [b["ContentID"] for b in books]
    marks = ",".join("?" * len(ids))
    chapters = db.execute(
        f"SELECT * FROM content WHERE ContentType IN ('9', '899') AND BookID IN ({marks})", ids
    ).fetchall()
    anon = Anonymizer(books, args.names.split(","), seed=args.device)

    # Keep the order of the reading dates, but not the dates themselves.
    by_date = sorted(books, key=lambda b: (b["DateLastRead"] or "", b["ContentID"]))
    last_read = {b["ContentID"]: f"2026-01-{n:02d}T12:00:00Z" for n, b in enumerate(by_date, 1) if b["DateLastRead"]}

    info = db.execute("PRAGMA table_info(content)").fetchall()
    columns = [c["name"] for c in info]
    # What SQLite fills in for a column left out of an INSERT.
    defaults = {c["name"]: db.execute(f"SELECT {c['dflt_value'] or 'NULL'}").fetchone()[0] for c in info}
    rows = []
    for row in list(books) + list(chapters):
        letter = anon.letters[row["ContentID"] if row["ContentType"] == "6" else row["BookID"]]
        out = anonymize_row(row, columns, anon, letter, last_read)
        key = (letter, row["ContentType"] != "6", int(row["ContentType"]), row["VolumeIndex"] or 0, out["ContentID"])
        rows.append((key, out))
    rows.sort(key=lambda r: r[0])

    bookmark_rows, bookmark_columns = [], []
    if args.bookmarks:
        bookmark_columns = [c[1] for c in db.execute("PRAGMA table_info(Bookmark)")]
        source = db.execute(f"SELECT * FROM Bookmark WHERE VolumeID IN ({marks}) ORDER BY BookmarkID", ids).fetchall()
        bookmark_rows = [anonymize_bookmark(row, bookmark_columns, anon) for row in source]
        bookmark_rows.sort(key=lambda r: (r["VolumeID"], r["ContentID"], r["StartContainerPath"], r["StartOffset"]))

    schema_names = ["DbVersion", "content"] + (["Bookmark"] if args.bookmarks else []) + [t for t in TOLINO_TABLES if db.execute(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?", (t,)).fetchone()]
    schema = []
    for name in schema_names:
        for (sql,) in db.execute(
            "SELECT sql FROM sqlite_master WHERE tbl_name = ? AND sql IS NOT NULL ORDER BY type DESC, name", (name,)
        ):
            schema.append(sql + ";")
    header = [
        f"-- {args.device}: sideloaded books and their chapters, anonymized.",
        "-- Generated by tools/make_db_fixture.py; do not edit by hand. See tests/fixtures/README.md.",
    ]
    schema_text = "\n".join(header + schema) + "\n"
    lines = [f"INSERT INTO DbVersion (version) VALUES ({v});" for (v,) in db.execute("SELECT version FROM DbVersion")]
    for _, out in rows:
        # Leave out columns that hold their default (same value and type), so
        # the rows stay readable; the round trip below proves nothing changed.
        cols = [c for c in columns if (type(out[c]), out[c]) != (type(defaults[c]), defaults[c])]
        lines.append(
            f"INSERT INTO content ({', '.join(cols)}) VALUES ({', '.join(sql_literal(out[c]) for c in cols)});"
        )
    for out in bookmark_rows:
        cols = [c for c in bookmark_columns if out[c] is not None]
        lines.append(f"INSERT INTO Bookmark ({', '.join(cols)}) VALUES ({', '.join(sql_literal(out[c]) for c in cols)});")
    text = schema_text + "\n".join(lines) + "\n"

    check_round_trip(text, [out for _, out in rows], columns)
    if args.bookmarks:
        check_round_trip(text, bookmark_rows, bookmark_columns, "Bookmark")
    strings, tokens = forbidden_strings(db, books, chapters)
    # A source that already holds placeholder books (the EPUB fixtures put on
    # a device) keeps them unchanged; their names are no leftover.
    placeholders = " ".join(
        f"{out['ContentID']} {out['Title']} {out['Attribution']}" for _, out in rows if out["ContentType"] == "6"
    )
    strings = {s for s in strings if s not in placeholders}
    tokens = {t for t in tokens if t.lower() not in placeholders.lower()}
    check_leftovers(text, schema_text, strings, tokens)
    with open(args.output, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    extra = f", {len(bookmark_rows)} highlights" if args.bookmarks else ""
    print(f"{args.output}: {len(books)} books, {len(chapters)} chapter rows{extra}, no leftovers found")


if __name__ == "__main__":
    main()
