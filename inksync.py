import os
import json
import sqlite3
from datetime import datetime
from dateutil import parser as dateparser
from pathlib import Path
from markdownify import markdownify as md

# Lade Konfiguration
with open("config.json") as f:
    config = json.load(f)

input_db = config["input_db"]
output_dir = config["output_dir"]
last_export_file = config["last_export_file"]
handwriting_tool_path = config.get("handwriting_tool_path")
export_handwriting = config.get("export_handwriting", False)

os.makedirs(output_dir, exist_ok=True)

# Lade letzten Exportzeitpunkt
if os.path.exists(last_export_file):
    with open(last_export_file) as f:
        last_export = json.load(f)
else:
    last_export = {"last_timestamp": "1970-01-01T00:00:00"}

last_ts = dateparser.parse(last_export["last_timestamp"])

# Verbinde zur SQLite DB
conn = sqlite3.connect(input_db)
c = conn.cursor()

# Hole Annotationen (Highlights & Notizen)
c.execute("""
    SELECT Bookmark.Text, Bookmark.DateCreated, Content.ContentID, Content.Title, Content.Attribution, Bookmark.Annotation
    FROM Bookmark
    JOIN Content ON Bookmark.ContentID = Content.ContentID
    WHERE Bookmark.DateCreated > ?
""", (last_ts.isoformat(),))

rows = c.fetchall()

books = {}
for text, created, content_id, title, author, annotation in rows:
    book_key = f"{title}_{author}".replace(" ", "_").replace("/", "-")
    if book_key not in books:
        books[book_key] = {
            "title": title,
            "author": author,
            "highlights": []
        }
    entry = {
        "text": text,
        "annotation": annotation,
        "created": created,
        "tag": "#highlight" if annotation is None else "#note"
    }
    books[book_key]["highlights"].append(entry)

# Optional: Handschriftliche Notizen aus externem Tool holen
if export_handwriting and handwriting_tool_path:
    handwriting_dir = Path(handwriting_tool_path) / "output"
    for file in handwriting_dir.glob("*.md"):
        with open(file) as f:
            content = f.read()
        # Einfacher Heuristik-Parser für Titel und Autor aus Dateiname
        name_parts = file.stem.split("_")
        if len(name_parts) >= 2:
            title = name_parts[0]
            author = "_".join(name_parts[1:])
            book_key = f"{title}_{author}".replace(" ", "_")
            if book_key not in books:
                books[book_key] = {
                    "title": title,
                    "author": author,
                    "highlights": []
                }
            books[book_key]["highlights"].append({
                "text": content.strip(),
                "annotation": None,
                "created": datetime.now().isoformat(),
                "tag": "#handwriting"
            })

# Exportiere Markdown-Dateien
for book_key, data in books.items():
    filename = Path(output_dir) / f"{book_key}.md"
    with open(filename, "w") as f:
        f.write("---\n")
        f.write(f"title: {data['title']}\n")
        f.write(f"author: {data['author']}\n")
        f.write(f"genre: Unknown\n")
        f.write(f"read_duration: Unknown\n")
        f.write("---\n\n")
        for item in sorted(data["highlights"], key=lambda x: x["created"]):
            f.write(f"- {item['tag']} {item['text'].strip()}\n")
            if item["annotation"]:
                f.write(f"  \> {item['annotation'].strip()}\n")
            f.write("\n")

# Speichere aktuellen Zeitstempel
now_ts = datetime.now().isoformat()
with open(last_export_file, "w") as f:
    json.dump({"last_timestamp": now_ts}, f)

print("✅ Export abgeschlossen. Markdown-Dateien befinden sich im Output-Ordner.")
