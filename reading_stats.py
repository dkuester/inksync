import sqlite3
from collections import defaultdict
from datetime import datetime
from pathlib import Path
import argparse

def parse_arguments():
    parser = argparse.ArgumentParser(description="Erstelle Lesestatistiken aus mehreren KoboReader.sqlite-Dateien.")
    parser.add_argument('--dbs', type=str, nargs='+', help="Pfad(e) zur(n) KoboReader.sqlite-Datei(en), getrennt durch Leerzeichen.")
    parser.add_argument('--output', type=str, default='reading_stats.md', help="Pfad zur Ausgabedatei (Markdown)")
    parser.add_argument('--filter', type=str, help="Optionaler Titel-Filter (Teilstring)")
    return parser.parse_args()

def fetch_reading_stats(db_path):
    conn = sqlite3.connect(db_path)
    cur = conn.cursor()
    cur.execute("""
        SELECT Title, TimeSpentReading, LastTimeStartedReading
        FROM content
        WHERE TimeSpentReading > 0 AND Title IS NOT NULL AND LastTimeStartedReading IS NOT NULL
    """)
    rows = cur.fetchall()
    conn.close()
    return rows

def group_stats(rows):
    week_stats = defaultdict(float)
    month_stats = defaultdict(float)
    year_stats = defaultdict(float)
    total_hours = 0.0
    book_hours = defaultdict(float)
    book_details = defaultdict(list)

    for title, minutes, last_read in rows:
        try:
            dt = datetime.fromisoformat(last_read.replace("Z", "+00:00"))
        except ValueError:
            continue
        hours = round(minutes / 60.0 / 60.0, 1)
        total_hours += hours
        book_hours[title] += hours
        book_details[title].append((dt.strftime("%Y-%m-%d"), hours))

        week_key = f"{dt.isocalendar().year}-KW{dt.isocalendar().week:02}"
        month_key = dt.strftime("%Y-%m")
        year_key = dt.strftime("%Y")

        week_stats[week_key] += hours
        month_stats[month_key] += hours
        year_stats[year_key] += hours

    return week_stats, month_stats, year_stats, total_hours, book_hours, book_details

def markdown_section(title, stats_dict):
    lines = [f"### {title}", "", "| Zeitraum | Stunden |", "|----------|---------|"]
    for key in sorted(stats_dict.keys()):
        lines.append(f"| {key} | {stats_dict[key]:.1f} |")
    return "\n".join(lines)

def markdown_top_books(book_hours, top_n=10):
    lines = [f"### Top {top_n} Bücher nach Lesezeit", "", "| Titel | Stunden |", "|-------|---------|"]
    for title, hours in sorted(book_hours.items(), key=lambda x: x[1], reverse=True)[:top_n]:
        lines.append(f"| {title} | {hours:.1f} |")
    return "\n".join(lines)

def markdown_book_details(book_details):
    lines = [f"### Lesestatistiken pro Buch", "", "| Titel | Datum | Stunden |", "|-------|------------|---------|"]
    for title, details in book_details.items():
        for date, hours in details:
            lines.append(f"| {title} | {date} | {hours:.1f} |")
    return "\n".join(lines)

def generate_markdown(week_stats, month_stats, year_stats, total_hours, book_hours, book_details):
    parts = [
        "# 📚 Lesestatistiken",
        "",
        f"**Gesamtlesezeit**: **{total_hours:.1f} Stunden**",
        "",
        markdown_section("Wöchentliche Übersicht", week_stats),
        "",
        markdown_section("Monatliche Übersicht", month_stats),
        "",
        markdown_section("Jährliche Übersicht", year_stats),
        "",
        markdown_top_books(book_hours),
        "",
        markdown_book_details(book_details),  # Neue Sektion für Lesestatistiken pro Buch
    ]
    return "\n".join(parts)

def main():
    args = parse_arguments()
    db_paths = args.dbs
    output_path = args.output
    title_filter = args.filter

    all_rows = []
    for db_path in db_paths:
        rows = fetch_reading_stats(db_path)
        all_rows.extend(rows)

    if title_filter:
        all_rows = [r for r in all_rows if title_filter.lower() in r[0].lower()]

    week_stats, month_stats, year_stats, total_hours, book_hours, book_details = group_stats(all_rows)
    markdown = generate_markdown(week_stats, month_stats, year_stats, total_hours, book_hours, book_details)

    with open(output_path, 'w', encoding='utf-8') as f:
        f.write(markdown)

    print(f"Lesestatistiken gespeichert unter: {output_path}")

if __name__ == "__main__":
    main()
