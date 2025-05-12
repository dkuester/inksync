# 📚 Kobo Lesestatistiken

Dieses Python-Skript erstellt Lesestatistiken aus einer oder mehreren KoboReader.sqlite-Dateien. Es aggregiert die Daten und bietet eine detaillierte Übersicht über deine Leseaktivitäten, einschließlich der wöchentlichen, monatlichen und jährlichen Statistiken sowie einer Auflistung der Top-Bücher nach Lesezeit.

## Funktionen

- **Daten aus mehreren KoboReader.sqlite-Dateien**: Das Skript kann mehrere Kobo-Datenbanken verarbeiten, um die Leseaktivitäten über verschiedene Geräte hinweg zu konsolidieren.
- **Lesestatistiken pro Buch**: Zeigt die Lesezeit für jedes Buch, sortiert nach der gesamten Lesezeit.
- **Übersicht über wöchentliche, monatliche und jährliche Lesezeiten**.
- **Filteroption**: Du kannst nach einem Teilstring im Titel der Bücher filtern, um nur bestimmte Bücher zu analysieren.

## Installation

1. Stelle sicher, dass Python 3.7 oder höher installiert ist.
2. Installiere die benötigten Python-Bibliotheken:

```bash
pip install sqlite3 argparse
```

### Optional:

Pandas für erweiterte Datenverarbeitung (falls gewünscht):

```
pip install pandas
```

# Verwendung

## Skript ausführen

Um das Skript auszuführen, gib den folgenden Befehl in deinem Terminal ein:

```
python reading_stats.py --dbs /Pfad/zu/KoboReader1.sqlite /Pfad/zu/KoboReader2.sqlite --output reading_stats.md
```

## Optionen:

`--dbs`: Eine oder mehrere KoboReader.sqlite-Dateien (mit Leerzeichen getrennt). Du kannst so viele Datenbanken angeben, wie du möchtest.  
`--output`: Der Pfad zur Ausgabedatei (Standard: reading_stats.md).  
`--filter`: Optionaler Filter für den Titel. Das Skript wird nur Bücher anzeigen, deren Titel den angegebenen Teilstring enthalten.  

Beispiel:

```
python reading_stats.py --dbs ~/KoboReader1.sqlite ~/KoboReader2.sqlite --output ~/Desktop/reading_stats.md --filter "Harry Potter"
```

## Ausgabe

Das Skript erzeugt eine Markdown-Datei mit folgendem Inhalt:

- **Gesamtlesezeit**: Zeigt die gesamte Lesezeit aller Bücher zusammen an.
- **Wöchentliche Übersicht**: Lesezeit pro Woche.
- **Monatliche Übersicht**: Lesezeit pro Monat.
- **Jährliche Übersicht**: Lesezeit pro Jahr.
- **Top 10 Bücher nach Lesezeit**: Eine Liste der 10 Bücher mit der größten Lesezeit.
- **Lesestatistiken pro Buch**: Eine detaillierte Liste der Lesezeit pro Buch mit Datum und Stunden.

Beispiel für die Ausgabe:

```
# 📚 Lesestatistiken

**Gesamtlesezeit**: **150.2 Stunden**

### Wöchentliche Übersicht

| Zeitraum | Stunden |
|----------|---------|
| 2025-KW01 | 5.2     |
| 2025-KW02 | 6.8     |

### Monatliche Übersicht

| Zeitraum | Stunden |
|----------|---------|
| 2025-01   | 20.3     |
| 2025-02   | 18.5     |

### Jährliche Übersicht

| Zeitraum | Stunden |
|----------|---------|
| 2025     | 150.2    |

### Top 10 Bücher nach Lesezeit

| Titel              | Stunden |
|--------------------|---------|
| Harry Potter       | 12.5    |
| Der Hobbit         | 8.9     |

### Lesestatistiken pro Buch

| Titel            | Datum       | Stunden |
|------------------|-------------|---------|
| Harry Potter     | 2025-01-01  | 1.5     |
| Der Hobbit       | 2025-01-03  | 2.0     |
```

