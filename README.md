# 📚 Inksync

Ein Toolset zur Analyse und Archivierung deiner Kobo-Lesedaten – bestehend aus zwei Komponenten:

1. **`inksync.py`**: Exportiert Highlights, Notizen und handschriftliche Annotationen aus der `KoboReader.sqlite` in Markdown-Dateien für Obsidian.
2. **`reading_stats.py`**: Generiert Lesestatistiken (Zeit, Verteilung, Top-Bücher) ebenfalls auf Basis der `KoboReader.sqlite`.

---

## 🔧 Installation

1. **Repository klonen:**

```bash
git clone https://github.com/dkuester/inksync.git
cd inksync
```

2. **Virtuelle Umgebung erstellen (optional, empfohlen):**

```bash
python3 -m venv venv
source venv/bin/activate  # Bei Windows: venv\Scripts\activate
```

3. **Abhängigkeiten installieren:**

```bash
pip install -r requirements.txt
```

---

## 📘 `inksync.py`: Annotationsexport für Obsidian

Dieses Tool extrahiert Annotationen (Textmarkierungen, Notizen, handschriftliche Notizen) aus der `KoboReader.sqlite` und exportiert sie in strukturierte Markdown-Dateien – ideal für ein Obsidian-Setup.

### 🔹 Funktionen

- Delta-Export: nur neue Annotationen werden verarbeitet
- YAML-Frontmatter (Titel, Autor, Genre, Lesedauer)
- Unterstützt Text- und handschriftliche Notizen
- Konfigurierbarer Exportpfad
- Dateinamenschema: `titel_autor.md`
- Markdown-Format kompatibel mit Dataview

### 🔹 Ausführung

```bash
python inksync.py
```

### 🔹 Konfiguration

Konfiguriert wird das Tool über die Datei `config.json`, z. B.:

```json
{
  "input_path": "~/inksync/input/KoboReader.sqlite",
  "output_path": "~/inksync/output/",
  "export_handwritten": true
}
```

---

## 📊 `reading_stats.py`: Lesestatistiken für Obsidian

Dieses Tool erstellt aus der `KoboReader.sqlite` eine Markdown-Datei mit Leseübersichten, inklusive:

- Gesamtlesezeit
- Wöchentliche, monatliche und jährliche Lesezeit
- Top-Bücher nach Lesezeit
- Lesestatistiken pro Buch

Ideal für ein Obsidian-Dashboard.

### 🔹 Ausführung

```bash
python reading_stats.py
```

### 🔹 Optionen

- `--db`: Pfad zur `KoboReader.sqlite` (optional)
- `--output`: Zielpfad für die Markdown-Datei (optional)
- `--filter`: Optionaler Titel-Filter (Teilstring)

Beispiel:

```bash
python reading_stats.py --db ~/Downloads/KoboReader.sqlite --filter "Murakami"
```

---

## 🔁 Mehrere Geräte unterstützen

Falls du mehrere Kobo-Geräte nutzt, kannst du alle `KoboReader.sqlite`-Dateien einzeln exportieren und zusammenführen:

```bash
cat stats1.md stats2.md > kombi_stats.md
```

Eine zukünftige Version von `reading_stats.py` könnte auch mehrere Datenbanken einlesen.

---

## 🗃️ Verzeichnisstruktur (Empfehlung)

```
inksync/
├── input/
│   └── KoboReader.sqlite
├── output/
│   └── *.md              # Exporte für Obsidian
├── inksync.py
├── reading_stats.py
├── config.json
├── requirements.txt
└── README.md
```

---

## ✅ Anforderungen

- Python 3.8+
- Abhängigkeiten: `pandas`, `python-dateutil` (siehe `requirements.txt`)

---

## 📂 Integration mit Obsidian

Die erzeugten Markdown-Dateien sind direkt in Obsidian nutzbar, z. B. mit Plugins wie:

- [Dataview](https://github.com/blacksmithgu/obsidian-dataview)
- [Obsidian Charts](https://github.com/zgrosser/obsidian-charts)

---

## ✨ Roadmap (Ideen)

- Unterstützung mehrerer Datenbanken in `reading_stats.py`
- Automatischer Sync mit Obsidian-Vault
- Visualisierung von Leseverläufen
- Statistiken nach Genre/Autor

---

## 📝 Lizenz

Lizenz: [CC BY-NC 4.0](https://creativecommons.org/licenses/by-nc/4.0/legalcode) – freie Nutzung für nicht-kommerzielle Zwecke mit Namensnennung.
