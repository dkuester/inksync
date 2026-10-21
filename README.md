<div align="center">

# inksync

**Turn your Kobo's highlights, notes and reading time into Markdown for Obsidian.**

[![Python](https://img.shields.io/badge/python-3.8%2B-informational)](#requirements)
[![License: CC BY-NC 4.0](https://img.shields.io/badge/license-CC%20BY--NC%204.0-lightgrey)](LICENSE)

[Quick start](#quick-start) · [Annotation export](#annotation-export-inksyncpy) · [Reading statistics](#reading-statistics-reading_statspy) · [Roadmap](#roadmap) · [Deutsch](README.de.md)

</div>

inksync is a pair of command-line tools that read the database of a Kobo
e-reader (`KoboReader.sqlite`) and write Markdown files you can drop into an
[Obsidian](https://obsidian.md) vault: one file per book with your highlights
and notes, and one overview of your reading time.

> [!NOTE]
> **Status:** early version, being reworked. A calibre plugin is in progress on
> the [`migrate-to-calibre-plugin`](https://github.com/dkuester/inksync/tree/migrate-to-calibre-plugin)
> branch. The generated Markdown is currently in German.

---

## Features

| Tool | What it does |
|---|---|
| **`inksync.py`** | Exports highlights and notes, one Markdown file per book, with YAML front matter (title, author, source such as Kobo Store, calibre or Onleihe) and tags `#highlight` / `#note` for Dataview. |
| **`reading_stats.py`** | Builds one Markdown file with total reading time, time per week, month and year, the top 10 books by reading time and a per-book table. Reads several databases at once, e.g. from more than one Kobo. |

Both tools only read the database; they never change it.

## Requirements

- Python 3.8 or newer
- A copy of `KoboReader.sqlite` from the hidden `.kobo/` folder of your Kobo
  (connect it over USB)

## Quick start

```bash
git clone https://github.com/dkuester/inksync.git
cd inksync
python3 -m venv .venv
source .venv/bin/activate        # Windows: .venv\Scripts\activate
pip install -r requirements.txt
mkdir -p input && cp /path/to/KOBOeReader/.kobo/KoboReader.sqlite input/
```

## Annotation export (`inksync.py`)

```bash
python inksync.py --db input/KoboReader.sqlite --output output/
```

| Option | Default | Meaning |
|---|---|---|
| `--db` | `~/inksync/input/KoboReader.sqlite` | Path to the Kobo database |
| `--output` | `~/inksync/output` | Folder for the Markdown files |

Each book becomes `<title>.md`. If a file with that name already exists, the
new one gets a numbered suffix (`<title>_2.md`) and `duplicate: true` in its
front matter, so earlier exports are never overwritten.

<details>
<summary><strong>Example output</strong></summary>

```markdown
---
title: Kafka am Strand
author: Haruki Murakami
genre: ''
lesedauer: ''
quelle: Kobo Store
statistik_verfügbar: true
---

#highlight #note
> A highlighted passage.

My note on it.

_Kapitelposition_: 42%
_Erstellt am_: 2025-04-01T20:15:00.000

---
```

</details>

## Reading statistics (`reading_stats.py`)

```bash
python reading_stats.py --dbs input/KoboReader.sqlite --output reading_stats.md
python reading_stats.py --dbs kobo1.sqlite kobo2.sqlite --filter "Murakami"
```

| Option | Default | Meaning |
|---|---|---|
| `--dbs` | *(required)* | One or more paths to Kobo databases, separated by spaces |
| `--output` | `reading_stats.md` | Output Markdown file |
| `--filter` | none | Only books whose title contains this text |

## Using it with Obsidian

Copy the output into your vault. The front matter and tags work well with
[Dataview](https://github.com/blacksmithgu/obsidian-dataview) and
[Obsidian Charts](https://github.com/phibr0/obsidian-charts).

## Roadmap

- calibre plugin (in progress)
- Delta export: only new annotations since the last run
- Export of handwritten notes
- Configuration file instead of command-line options
- English Markdown output
- Statistics by genre and author, charts of the reading history

## Support

| | |
|---|---|
| Bug reports and ideas | [Issues](https://github.com/dkuester/inksync/issues). Please include your Kobo model and firmware version. |
| Contributing | Pull requests against `main`. |

## License

[CC BY-NC 4.0](LICENSE): free to use, share and adapt for non-commercial
purposes, with attribution.
