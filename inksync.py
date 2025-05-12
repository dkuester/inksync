import sqlite3
import os
import re
import argparse
import yaml
from datetime import datetime
from pathlib import Path

def parse_arguments():
    parser = argparse.ArgumentParser(description="Exportiere Annotationen aus KoboReader.sqlite")
    parser.add_argument("--db", default="~/inksync/input/KoboReader.sqlite", help="Pfad zur SQLite-Datenbank")
    parser.add_argument("--output", default="~/inksync/output", help="Zielordner für Markdown-Dateien")
    return parser.parse_args()

def sanitize_filename(text):
    if not isinstance(text, str) or text is None:
        text = "Unbekannt"
    return re.sub(r"[\\/*?\[\]:;]", "", text)

def detect_source_and_stats(content_id: str, content_type: int):
    if content_id.startswith("file:"):
        quelle = "Calibre"
        statistik = content_type == 6  # KEPUB
    elif content_id.startswith("library:"):
        quelle = "Onleihe"
        statistik = False
    elif content_id.startswith("book:"):
        quelle = "Kobo Store"
        statistik = True
    else:
        quelle = "Unbekannt"
        statistik = False
    return quelle, statistik

def fetch_annotations(db_path):
    conn = sqlite3.connect(os.path.expanduser(db_path))
    conn.row_factory = sqlite3.Row
    cur = conn.cursor()

    cur.execute("""
        SELECT
            b.BookmarkID,
            b.ContentID,
            b.Text,
            b.Annotation,
            b.DateCreated,
            b.ChapterProgress,
            c.Title,
            c.Attribution,
            c.ContentType
        FROM Bookmark b
        JOIN content c ON b.ContentID = c.ContentID
        WHERE b.Text IS NOT NULL OR b.Annotation IS NOT NULL
        ORDER BY c.Title, b.DateCreated
    """)

    annotations = {}
    for row in cur.fetchall():
        book_key = (row["Title"], row["Attribution"], row["ContentID"], row["ContentType"])
        if book_key not in annotations:
            annotations[book_key] = []
        annotations[book_key].append({
            "highlight": row["Text"],
            "note": row["Annotation"],
            "chapter_progress": row["ChapterProgress"],
            "date": row["DateCreated"],
        })

    conn.close()
    return annotations

def export_annotations(annotations, output_dir):
    os.makedirs(output_dir, exist_ok=True)
    existing_filenames = set()

    for (title, author, content_id, content_type), items in annotations.items():
        raw_title = title or "Unbekannt"
        raw_author = author or "Unbekannt"
        safe_title = sanitize_filename(raw_title)
        base_filename = safe_title
        filename = f"{base_filename}.md"
        counter = 2
        is_duplicate = False

        while filename in existing_filenames or os.path.exists(os.path.join(output_dir, filename)):
            filename = f"{base_filename}_{counter}.md"
            counter += 1
            is_duplicate = True

        existing_filenames.add(filename)
        output_path = os.path.join(output_dir, filename)

        quelle, statistik_verfügbar = detect_source_and_stats(content_id, content_type)

        frontmatter = {
            "title": raw_title,
            "author": raw_author,
            "genre": "",
            "lesedauer": "",
            "quelle": quelle,
            "statistik_verfügbar": statistik_verfügbar,
        }

        if is_duplicate:
            frontmatter["duplicate"] = True

        with open(output_path, "w", encoding="utf-8") as f:
            f.write("---\n")
            yaml.dump(frontmatter, f, allow_unicode=True, sort_keys=False)
            f.write("---\n\n")

            for item in items:
                tags = []
                if item["highlight"]:
                    tags.append("#highlight")
                if item["note"]:
                    tags.append("#note")
                tag_str = " ".join(tags)

                f.write(f"{tag_str}\n")
                if item["highlight"]:
                    f.write(f"> {item['highlight'].strip()}\n\n")
                if item["note"]:
                    f.write(f"{item['note'].strip()}\n\n")
                if item["chapter_progress"] is not None:
                    f.write(f"_Kapitelposition_: {round(item['chapter_progress'] * 100)}%\n")
                f.write(f"_Erstellt am_: {item['date']}\n\n")
                f.write("---\n\n")

        print(f"✔ Exportiert: {filename}")

def main():
    args = parse_arguments()
    annotations = fetch_annotations(args.db)
    export_annotations(annotations, os.path.expanduser(args.output))

if __name__ == "__main__":
    main()
