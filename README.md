# InkSync – Kobo Annotation Export Tool

InkSync ist ein Python-basiertes Tool zum Extrahieren und Exportieren von Notizen und Markierungen aus der `KoboReader.sqlite`-Datenbank deines Kobo e-Readers. Die Ergebnisse werden als Markdown-Dateien ausgegeben und können direkt in Obsidian verwendet werden.

## 📦 Projektstruktur
```
inksync/
├── inksync.py              # Hauptskript
├── config.json             # Konfigurationsdatei
├── requirements.txt        # Python-Abhängigkeiten
├── .gitignore              # Dateien für Git ignorieren
├── README.md               # Diese Datei
├── input/                  # Hier kommt die KoboReader.sqlite rein
└── output/                 # Exportierte Markdown-Dateien
```

## ⚙️ Konfiguration
Passe `config.json` an, um Pfade und Optionen zu steuern:
```json
{
  "input_db": "input/KoboReader.sqlite",
  "output_dir": "output",
  "last_export_file": "last_export.json",
  "handwriting_tool_path": "tools/kobo-markup-generator",
  "export_handwriting": true
}
```

## 🚀 Nutzung
1. Kopiere deine `KoboReader.sqlite` in den Ordner `input/`.
2. Installiere die Abhängigkeiten:
   ```bash
   pip install -r requirements.txt
   ```
3. Starte das Tool:
   ```bash
   python inksync.py
   ```

Die Markdown-Dateien werden im `output/`-Verzeichnis erstellt.

## 📓 Hinweise
- Das Tool merkt sich den letzten Exportzeitpunkt und exportiert nur neue Annotationen (Delta-Export).
- Handschriftliche Notizen können mit dem Tool [Unofficial Kobo Composite Markup Generator](https://github.com/leldr/Unofficial-Chrome-Based-Kobo-Composite-Markup-Generator) integriert werden.

---
