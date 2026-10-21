<div align="center">

# inksync

**Markierungen, Notizen und Lesezeit vom Kobo als Markdown für Obsidian.**

[![Python](https://img.shields.io/badge/python-3.8%2B-informational)](#voraussetzungen)
[![Lizenz: CC BY-NC 4.0](https://img.shields.io/badge/license-CC%20BY--NC%204.0-lightgrey)](LICENSE)

[Schnellstart](#schnellstart) · [Annotationsexport](#annotationsexport-inksyncpy) · [Lesestatistik](#lesestatistik-reading_statspy) · [Roadmap](#roadmap) · [English](README.md)

</div>

inksync besteht aus zwei Kommandozeilen-Werkzeugen, die die Datenbank eines
Kobo-E-Readers (`KoboReader.sqlite`) lesen und Markdown-Dateien für einen
[Obsidian](https://obsidian.md)-Vault schreiben: eine Datei pro Buch mit deinen
Markierungen und Notizen und eine Übersicht deiner Lesezeit.

> [!NOTE]
> **Stand:** frühe Version, wird überarbeitet. Ein calibre-Plugin entsteht im
> Branch [`migrate-to-calibre-plugin`](https://github.com/dkuester/inksync/tree/migrate-to-calibre-plugin).

---

## Funktionen

| Werkzeug | Was es tut |
|---|---|
| **`inksync.py`** | Exportiert Markierungen und Notizen, eine Markdown-Datei pro Buch, mit YAML-Frontmatter (Titel, Autor, Quelle wie Kobo Store, calibre oder Onleihe) und den Tags `#highlight` / `#note` für Dataview. |
| **`reading_stats.py`** | Erzeugt eine Markdown-Datei mit Gesamtlesezeit, Lesezeit pro Woche, Monat und Jahr, den 10 Büchern mit der meisten Lesezeit und einer Tabelle pro Buch. Liest auch mehrere Datenbanken auf einmal, z. B. von mehreren Kobos. |

Beide Werkzeuge lesen die Datenbank nur und verändern sie nie.

## Voraussetzungen

- Python 3.8 oder neuer
- Eine Kopie von `KoboReader.sqlite` aus dem versteckten Ordner `.kobo/` deines
  Kobo (per USB verbinden)

## Schnellstart

```bash
git clone https://github.com/dkuester/inksync.git
cd inksync
python3 -m venv .venv
source .venv/bin/activate        # Windows: .venv\Scripts\activate
pip install -r requirements.txt
mkdir -p input && cp /pfad/zu/KOBOeReader/.kobo/KoboReader.sqlite input/
```

## Annotationsexport (`inksync.py`)

```bash
python inksync.py --db input/KoboReader.sqlite --output output/
```

| Option | Standard | Bedeutung |
|---|---|---|
| `--db` | `~/inksync/input/KoboReader.sqlite` | Pfad zur Kobo-Datenbank |
| `--output` | `~/inksync/output` | Ordner für die Markdown-Dateien |

Jedes Buch wird zu `<titel>.md`. Gibt es schon eine Datei dieses Namens,
bekommt die neue eine Nummer (`<titel>_2.md`) und `duplicate: true` im
Frontmatter; frühere Exporte werden also nie überschrieben.

<details>
<summary><strong>Beispielausgabe</strong></summary>

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
> Eine markierte Textstelle.

Meine Notiz dazu.

_Kapitelposition_: 42%
_Erstellt am_: 2025-04-01T20:15:00.000

---
```

</details>

## Lesestatistik (`reading_stats.py`)

```bash
python reading_stats.py --dbs input/KoboReader.sqlite --output reading_stats.md
python reading_stats.py --dbs kobo1.sqlite kobo2.sqlite --filter "Murakami"
```

| Option | Standard | Bedeutung |
|---|---|---|
| `--dbs` | *(Pflicht)* | Ein oder mehrere Pfade zu Kobo-Datenbanken, durch Leerzeichen getrennt |
| `--output` | `reading_stats.md` | Ausgabedatei (Markdown) |
| `--filter` | keiner | Nur Bücher, deren Titel diesen Text enthält |

## Mit Obsidian nutzen

Die Ausgabe in den Vault kopieren. Frontmatter und Tags passen gut zu
[Dataview](https://github.com/blacksmithgu/obsidian-dataview) und
[Obsidian Charts](https://github.com/phibr0/obsidian-charts).

## Roadmap

- calibre-Plugin (in Arbeit)
- Delta-Export: nur neue Annotationen seit dem letzten Lauf
- Export handschriftlicher Notizen
- Konfigurationsdatei statt Kommandozeilen-Optionen
- Englische Markdown-Ausgabe
- Statistiken nach Genre und Autor, Diagramme des Leseverlaufs

## Support

| | |
|---|---|
| Fehler und Ideen | [Issues](https://github.com/dkuester/inksync/issues). Bitte Kobo-Modell und Firmware-Version angeben. |
| Mitwirken | Pull-Requests gegen `main`. |

## Lizenz

[CC BY-NC 4.0](LICENSE): freie Nutzung, Weitergabe und Bearbeitung für
nicht-kommerzielle Zwecke mit Namensnennung.
