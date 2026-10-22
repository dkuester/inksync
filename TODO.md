# Arbeitsliste NickelInkSync

Konkrete Schritte aus der [Roadmap](ROADMAP.md), in der Reihenfolge, in der wir
sie angehen. Ein Branch und ein PR pro Punkt (oder pro zusammengehörender
Gruppe), ein Commit pro erledigtem Schritt. Nach einer Unterbrechung: diese
Datei und `git status` / `git log` lesen, dann beim ersten offenen Punkt
weitermachen.

Status: `[ ]` offen · `[~]` in Arbeit · `[x]` erledigt · `[!]` blockiert (Grund steht dabei)

Wer: **du** = braucht dich (Gerät, Entscheidung, Zugang) · **ich** = kann ich allein erledigen

Erledigte Punkte wandern in der Roadmap nach „Recently completed“.

## Grundlagen

- [x] **G1 Lizenz** · PR #5
  - [x] **du:** Lizenz wählen: MIT, wie NickelHook und deine anderen Projekte.
    KOReader, koreader-sync-server und BookOrbit stehen unter AGPL-3.0; ihr
    Code dient nur als Vorlage, nachgebaut wird das beschriebene Verhalten.
  - [x] **ich:** `LICENSE` ersetzen, Abschnitt „Lizenz“ in beiden READMEs
    anpassen, Lizenzhinweis in `SCOPE.md`.
- [x] **G2 Rust-Grundgerüst** · PR #6
  - [x] **ich:** Crate `nickelinksync` (Bibliothek + Programm), `rustfmt.toml`,
    `clippy` ohne Warnungen, `Cargo.lock` im Repo.
  - [x] **ich:** CI-Job `check` um `cargo fmt --check`, `cargo clippy` und
    `cargo test` erweitern; die beiden bestehenden Prüfungen bleiben, der
    Jobname bleibt `check`.
- [x] **G3 ARM-Build** · PR #8
  - [x] **ich:** Statisch gelinktes Programm für
    `armv7-unknown-linux-musleabihf`, gebaut mit `cargo-zigbuild`
    (`just build-device`). Es hängt nicht von der glibc des Geräts ab und
    läuft daher auf Firmware 4.x und 5.x; `strayrose/nickeltc` bleibt für das
    Plugin (N2/N4). Der CI-Job `check` baut es, prüft es mit `file` und
    `qemu-arm` und lädt es als Artefakt `nickelinksync-armv7-musleabihf` hoch.
  - [x] **du:** Testlauf auf Clara BW (4.46) und Shine Color (5.18): Programm aus
    dem CI-Artefakt nach `.adds/NickelInkSync/` kopiert, `--version` gibt
    `nickelinksync 0.1.0` aus.
- [x] **G4 Testdaten** · PR #9
  - [x] **ich:** Skript, das aus einer `KoboReader.sqlite` nur die nötigen
    Tabellen und Zeilen übernimmt (seitengeladene Bücher und ihre Kapitel) und
    Titel, Autoren und Pfade ersetzt (`tools/make_db_fixture.py`), dazu
    Platzhalter-EPUBs mit der Struktur der drei Bücher
    (`tools/make_epub_fixture.py`) und ein Test, der die Fixtures lädt. Siehe
    [`tests/fixtures/README.md`](tests/fixtures/README.md).
  - [x] **du:** Gekürzte Daten vom Clara BW und Shine Color durchgesehen und
    freigegeben, bevor sie ins Repo kamen. Es ging um deine Lesedaten.

## Kern

- [ ] **K1 Gerätedatenbank lesen**
  - [ ] **ich:** Nur lesend öffnen (`immutable=1` für Kopien, `mode=ro` am Gerät).
  - [ ] **ich:** Seitengeladene Bücher auflisten (`ContentType = 6`,
    `file:///mnt/onboard/`), Position aus `ChapterIDBookmarked`,
    `___PercentRead`, `DateLastRead` lesen.
  - [ ] **ich:** Kapitel über die OPF-Spine und den `href` aus
    `ChapterIDBookmarked` finden (relativ zur EPUB-Wurzel), nicht über das
    Format der Kapitel-`ContentID` (Kobo: Pfad, Tolino: UUID). `BookID` +
    `VolumeIndex` nur auf dem Kobo als Gegenprobe (`ContentType = 9` =
    Spine-Index); auf dem Tolino sind die Kapitelzeilen ToC-Einträge,
    `VolumeIndex` ist dort der ToC-Index (siehe `SCOPE.md`).
  - [ ] **ich:** Firmware aus `.kobo/version` lesen (drittes Feld), Tolino an der
    Datenbank erkennen (`tolino_*`-Tabellen).
- [ ] **K2 Prüfsumme**
  - [ ] **ich:** Teil-MD5 wie KOReader (1-KiB-Stichproben ab Offset 256).
  - [x] **du:** `metadata.epub.lua` aus KOReader geliefert, für die drei
    Fixture-EPUBs (`tests/fixtures/koreader/`). Sie enthalten
    `partial_md5_checksum` als Vergleichswert und die Positionen für K4.
  - [ ] **ich:** Test gegen diese Werte.
- [ ] **K3 Position auf Kapitelebene**
  - [ ] **ich:** `ChapterIDBookmarked` zerlegen: `href#kobo.N.M` (kepub),
    `href#point(...)` (EPUB) und `href` ohne Fragment (Kapitelanfang).
  - [ ] **ich:** OPF-Spine lesen, `href` → `DocFragment[n]`, Prozentwert als
    Rückfallebene.
  - [ ] **ich:** Tests mit den Fixtures aus G4: Heat 2 (Kobo und Tolino:
    `xhtml-0-81.xhtml` = Spine-Index 81 → `DocFragment[82]`; auf dem Tolino
    hat die Kapitelzeile `VolumeIndex` 80) und Titan 2 (Spine-Index 39, ohne
    ToC-Eintrag und ohne Kapitelzeile).
- [ ] **K4 Position genau**
  - [x] **ich:** Aus der Datei ablesen, welchen DOM-Pfad KOReader erwartet
    (Leerraum, `koboSpan`-Elemente): KOReader behält `koboSpan` und die
    kepub-Hüllen, fasst Leerraum zusammen und zählt Zeichen; der Adobe-Point
    zählt UTF-8-Bytes (`SCOPE.md`).
  - [ ] **ich:** Kobo-Span → XPointer und Adobe-Point → XPointer, jeweils auch
    rückwärts.
  - [x] **du:** Book P (`tests/fixtures/epub/book-p*.epub`) auf Kobo, Tolino
    und in KOReader laden und die acht Zielwörter markieren (Liste in
    `tests/fixtures/README.md`), dann beide Datenbanken und die `.sdr`-Ordner
    kopieren.
  - [x] **ich:** Markierungen auslesen (`make_db_fixture.py --bookmarks`) und
    mit den erwarteten Offsets vergleichen (Ergebnis in
    `tests/fixtures/README.md`).
  - [ ] **du:** Gegenprobe: Ein von uns berechneter XPointer öffnet in KOReader
    die richtige Stelle.
- [ ] **K5 KOSync-Client**
  - [ ] **ich:** Lokalen koreader-sync-server per Docker für Tests aufsetzen.
  - [ ] **ich:** Anmeldung (`x-auth-user`, `x-auth-key` = MD5 des Passworts),
    Fortschritt lesen (`GET`) und schreiben (`PUT`).
  - [ ] **du:** Adresse deines eigenen Servers und ein Testkonto, oder die
    Entscheidung, vorerst nur lokal zu testen.

## Programm

- [ ] **P1 `status`, `push`, `pull`**
  - [ ] **ich:** Konfigurationsdatei (Server, Benutzer, Gerätename); das
    Passwort steht nicht in der Befehlszeile.
  - [ ] **ich:** `status`: Position auf dem Gerät und auf dem Server gegenüberstellen.
  - [ ] **ich:** `push`: Position zum Server.
  - [ ] **ich:** `pull`: nur anzeigen, was sich ändern würde (Probelauf als
    Standard). Die neueste Position gewinnt; rückwärts nur nach Rückfrage.
  - [ ] **du:** Ein Durchlauf mit Kobo, Tolino und KOReader am selben Buch.
- [ ] **P2 Rückschreiben per USB**
  - [ ] **ich:** `pull --write`: Sicherungskopie der Datenbank, dann
    `ChapterIDBookmarked` und `___PercentRead` setzen.
  - [ ] **du:** Gerät auswerfen und prüfen, ob Nickel die neue Position
    übernimmt oder überschreibt.

## Plugin

- [ ] **N1 Symbole für Kobo 4.46 (Qt5)**
  - [ ] **du:** Firmware-Update 4.46 für den Clara BW besorgen (wegen `libnickel.so`).
  - [ ] **ich:** Symbole für „Buch geöffnet“, „Buch geschlossen“ und „an
    Position springen“ suchen.
- [ ] **N2 Plugin Qt5**
  - [ ] **ich:** NickelHook-Plugin, das das Programm beim Schließen (`push`)
    und Öffnen (`pull`) aufruft; Paket `KoboRoot.tgz`.
  - [ ] **du:** Test auf dem Clara BW.
- [ ] **N3 Symbole für Tolino 5.18 (Qt6)**
  - [ ] **du:** Firmware-Update 5.18 für den Shine Color besorgen.
  - [ ] **ich:** Dieselben Symbole in der Qt6-Fassung suchen.
- [ ] **N4 Plugin Qt6**
  - [ ] **ich:** Qt6-Build, Paket `Kobo.tgz`.
  - [ ] **du:** Test auf dem Shine Color.

## Optional (später)

- [ ] **O1 Manuelle Zuordnung** von Büchern (kepub ↔ EPUB), Testfall Heat 2.
  Vorbild: BookOrbit, nur als Konzept (AGPL-3.0).
- [x] **O2 Normales EPUB auf Kobo 4.x** · **du:** ein normales EPUB auf den Clara
  geladen und geöffnet. Ergebnis: wie auf dem Tolino `#point(...)`,
  Kapitelzeilen sind ToC-Einträge (`SCOPE.md`).
- [ ] **O3 Einstellungsdialog** auf dem Gerät.
- [ ] **O4 Sync-Status** im Buchmenü, plus „Jetzt synchronisieren“.
- [ ] **O5 Geräteübersicht** in `status`: alle Positionen eines Buchs nebeneinander.
