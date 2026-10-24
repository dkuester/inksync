# Arbeitsliste NickelInkSync

Konkrete Schritte aus der [Roadmap](ROADMAP.md), in der Reihenfolge, in der wir
sie angehen. Ein Branch und ein PR pro Punkt (oder pro zusammengehörender
Gruppe), ein Commit pro erledigtem Schritt. Nach einer Unterbrechung: diese
Datei und `git status` / `git log` lesen, dann beim ersten offenen Punkt
weitermachen.

Status: `[ ]` offen · `[~]` in Arbeit · `[x]` erledigt · `[!]` blockiert (Grund steht dabei)

Wer: **du** = braucht dich (Gerät, Entscheidung, Zugang) · **ich** = kann ich allein erledigen

Erledigte Punkte wandern in der Roadmap nach „Recently completed“.

**Reihenfolge der Geräte:** Erst wird alles, was mit dem **Tolino Shine Color
(5.18)** allein geht, umgesetzt und am Gerät getestet: P3, K4 rückwärts für
normale EPUBs, P2 am Tolino, N3, N4. Danach kommt der **Kobo** (Clara BW,
4.46) in einem eigenen Durchgang: P1-Durchlauf mit allen Geräten,
K4 rückwärts für kepub (`#kobo.N.M`), P2 am Kobo, N1, N2. So geraten die
beiden Geräte beim Testen nicht durcheinander.

**Danach zuerst D1, dann D2:** Sobald der Tolino-Durchgang abgeschlossen
ist, kommt die **manuelle Buchzuordnung in der CLI (D1)**, danach der
**Client mit grafischer Oberfläche (D2)**, mit höchster Priorität und vor
allem anderen (auch vor dem Kobo-Durchgang). Ohne die Zuordnung gibt es
weder den Sync kepub ↔ EPUB noch den Kindle-Sync. Wartung und Fehlerbehebung
dürfen jederzeit dazwischengeschoben werden.

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

## Desktop-Client

- [ ] **D1 CLI mit manueller Buchzuordnung** · höchste Priorität nach dem
  Tolino-Durchgang. Löst O1 ab und entblockt den Sync kepub ↔ EPUB und den
  Kindle-Sync (L1).
  - [ ] **ich:** Zuordnungsspeicher: Buch auf dem Gerät (kepub, EPUB unter
    anderem Hash, KFX) ↔ EPUB und KOReader-Prüfsumme; Testfall Heat 2.
  - [ ] **ich:** Befehle zum Anlegen, Anzeigen und Löschen einer Zuordnung;
    Vorschläge aus Titel und Autor (beim Kindle auch `content_id`).
  - [ ] **ich:** `status`/`push`/`pull` nutzen die Zuordnung; Positionen
    werden in der zugeordneten EPUB übersetzt.
- [ ] **D2 Desktop-Client mit grafischer Oberfläche** · direkt nach D1.
  Baut auf derselben Bibliothek auf: Geräte und Bücher anzeigen, Zuordnung
  anlegen, Sync auslösen.
  - [x] **du:** Plattformen festgelegt: zuerst **macOS**, dann **Windows**,
    Linux optional, dann für möglichst viele Distributionen und Versionen.
  - [x] **du:** Funktionsumfang der ersten Version festgelegt (2026-10-24):
    - **Geräte:** Tolino und Kobo (lesen, zurückschreiben, sobald es in der
      CLI fertig ist), Kindle nur lesen (Position zum Server).
    - **Sync:** Positionen anzeigen, Push/Pull per Knopf mit Rückfrage nach
      der `confirm`-Einstellung, wie in der CLI. Kein automatischer Sync
      beim Anstecken.
    - **Zuordnung:** Vorschläge nach Titel und Autor (Kindle: `content_id`),
      du bestätigst oder wählst selbst. Bücher ohne Zuordnung und ohne
      gleiche Prüfsumme stehen in einer Liste „Nicht zugeordnet“ und werden
      nicht synchronisiert.
    - **EPUB-Originale:** aus einem konfigurierbaren Bibliotheksordner, der
      **Calibre-Bibliothek** (nur lesend, `metadata.db` als unveränderliche
      SQLite-Datei).
    - **Bücher aufs Gerät kopieren:** aus der Calibre-Bibliothek auf Tolino
      oder Kobo. Beim Kobo bietet die App an, das Buch mit **kepubify**
      (MIT) nach kepub umzuwandeln. Beim Kopieren legt die App die
      Zuordnung selbst an (kepub ↔ Original-EPUB), weil sie beide Dateien
      kennt; das Tolino-EPUB bleibt unverändert und hat dieselbe Prüfsumme.
    - **Einstellungen:** Dialog für Server, Benutzer, Passwort, Gerätename
      und `confirm`; das Passwort liegt im Schlüsselbund (macOS) bzw. in
      der Windows-Anmeldeinformationsverwaltung, die Datei hält nur den
      Benutzernamen.
    - **Gemeinsame Daten:** CLI und GUI teilen Konfiguration und
      Zuordnungen.
    - **Oberfläche:** normales Fenster, Deutsch und Englisch nach
      Systemsprache, umschaltbar.
    - **Außerdem:** Geräteansicht nebeneinander (O5), Sicherungskopie der
      Gerätedatenbank per Knopf zurückspielen, Verlauf der Syncs,
      Hinweis auf ein neues Release (keine Selbstaktualisierung).
  - [x] **du:** kepubify wird **mitgeliefert**: ein Binary je Plattform
    (macOS, Windows, Linux im AppImage), mit dem MIT-Lizenzhinweis von
    kepubify in der App und im Paket.
  - [ ] **ich:** kepubify-Binaries in der CI laden (feste Version, Prüfsumme
    gegen das Release prüfen) und ins Paket legen; Lizenzhinweis in
    „Über“ und neben dem Binary.
  - [ ] **ich:** GUI mit **egui/eframe** (reines Rust, MIT/Apache, ohne
    WebView, ein Code für alle drei Systeme).
  - [ ] **ich:** macOS: `.app`-Bundle als `.dmg`, aus der CI (macOS-Runner);
    ohne Apple Developer ID nicht notarisiert (erster Start per
    Rechtsklick → „Öffnen“).
  - [ ] **ich:** Windows: `.exe` aus der CI (Windows-Runner); ohne
    Code-Signing warnt SmartScreen beim ersten Start.
  - [ ] **ich (optional):** Linux als **AppImage**, gebaut auf einer alten
    glibc-Basis (ältestes noch unterstütztes Ubuntu LTS), damit es auf
    möglichst vielen Distributionen und Versionen läuft; keine Abhängigkeit
    außer Grafik- und Fensterbibliotheken (X11/Wayland, OpenGL), die jede
    Desktop-Installation hat. Ein statischer musl-Build geht für die GUI
    nicht, weil die Grafiktreiber zur Laufzeit geladen werden.

## Kern

- [x] **K1 Gerätedatenbank lesen**
  - [x] **ich:** Nur lesend öffnen: `immutable=1` für Kopien und per USB
    angeschlossene Geräte (`db::DeviceDb::open_snapshot`, am Tolino geprüft:
    Datei unverändert, keine Journal-Dateien); `mode=ro` auf dem Gerät,
    während Nickel läuft (`--live`, `db::DeviceDb::open_live`).
  - [x] **ich:** Seitengeladene Bücher auflisten (`ContentType = 6`,
    `file:///mnt/onboard/`), Position aus `ChapterIDBookmarked`,
    `___PercentRead`, `DateLastRead` lesen.
  - [x] **ich:** Kapitel über die OPF-Spine und den `href` aus
    `ChapterIDBookmarked` finden (relativ zur EPUB-Wurzel), nicht über das
    Format der Kapitel-`ContentID` (Kobo: Pfad, Tolino: UUID). `BookID` +
    `VolumeIndex` nur auf dem Kobo als Gegenprobe (`ContentType = 9` =
    Spine-Index); auf dem Tolino sind die Kapitelzeilen ToC-Einträge,
    `VolumeIndex` ist dort der ToC-Index (siehe `SCOPE.md`).
  - [x] **ich:** Firmware aus `.kobo/version` lesen (drittes Feld), Tolino an der
    Datenbank erkennen (`tolino_*`-Tabellen). Dazu das Modell aus dem letzten
    Feld (bekannt: Clara BW, Shine Color).
- [x] **K2 Prüfsumme**
  - [x] **ich:** Teil-MD5 wie KOReader: 1-KiB-Stichproben ab Offset **0**
    (nicht 256), dann 1 KiB, 4 KiB, … (`hash::partial_md5`).
  - [x] **du:** `metadata.epub.lua` aus KOReader geliefert, für die drei
    Fixture-EPUBs (`tests/fixtures/koreader/`). Sie enthalten
    `partial_md5_checksum` als Vergleichswert und die Positionen für K4.
  - [x] **ich:** Test gegen diese Werte (alle sieben EPUB-Fixtures, `tests/device.rs`).
- [x] **K3 Position auf Kapitelebene**
  - [x] **ich:** `ChapterIDBookmarked` zerlegen: `href#kobo.N.M` (kepub),
    `href#point(...)` (EPUB) und `href` ohne Fragment (Kapitelanfang)
    (`position::Position::parse`).
  - [x] **ich:** OPF-Spine lesen, `href` → `DocFragment[n]` (`epub::Epub`).
    Prozentwert für KOSync: `___PercentRead / 100`.
  - [x] **ich:** Tests mit den Fixtures aus G4: Heat 2 (Kobo und Tolino:
    `xhtml-0-81.xhtml` = Spine-Index 81 → `DocFragment[82]`; auf dem Tolino
    hat die Kapitelzeile `VolumeIndex` 80) und Titan 2 (Spine-Index 39, ohne
    ToC-Eintrag und ohne Kapitelzeile).
- [~] **K4 Position genau**
  - [x] **ich:** Aus der Datei ablesen, welchen DOM-Pfad KOReader erwartet
    (Leerraum, `koboSpan`-Elemente): KOReader behält `koboSpan` und die
    kepub-Hüllen, fasst Leerraum zusammen und zählt Zeichen; der Adobe-Point
    zählt UTF-8-Bytes (`SCOPE.md`).
  - [x] **ich:** Kobo-Span → XPointer und Adobe-Point → XPointer
    (`position::to_xpointer`): alle 48 Markierungen von Kobo und Tolino in
    Book P und Book Q ergeben genau KOReaders XPointer.
  - [x] **ich:** Rückwärts für den Tolino: XPointer → Adobe-Point
    (`position::from_xpointer`). Rundreise: alle 16 Tolino-Markierungen in
    Book P und Book Q kommen unverändert zurück, KOReaders Markierungen
    derselben Wörter ergeben genau die Tolino-Points, ebenso Book R. `pull`
    zeigt im Probelauf, was auf dem Gerät gespeichert würde (`on device`).
  - [x] **du:** Am Tolino geprüft (2026-10-24): `pull --force` übersetzt
    KOReaders Server-Position in Book R (Anfang R2-63) in
    `OEBPS/text/chapter2.xhtml#point(/1/4/128/1:0)`, wie erwartet. Ob
    Nickel dort öffnet, prüft P2.
  - [ ] **ich (Kobo-Durchgang):** Rückwärts für den Kobo: XPointer →
    Kobo-Span (kepub) und Adobe-Point für 4.x (Text um Kommentare
    zusammengefasst).
  - [x] **ich:** Zweites Probebuch für Umlaute, Zeichen mit 3 und 4 Bytes,
    CDATA, Leerraum am Absatzanfang und kombinierende Akzente, unter eigenem
    Namen (**Book Q**, `book-q*.epub`), damit es nicht mit Book P verwechselt
    wird.
  - [x] **du:** Book Q auf Kobo, Tolino und in KOReader laden und die acht
    Zielwörter markieren (Liste in `tests/fixtures/README.md`). Auf dem Kobo
    erst im zweiten Anlauf vollständig gespeichert.
  - [x] **du:** Book P (`tests/fixtures/epub/book-p*.epub`) auf Kobo, Tolino
    und in KOReader laden und die acht Zielwörter markieren (Liste in
    `tests/fixtures/README.md`), dann beide Datenbanken und die `.sdr`-Ordner
    kopieren.
  - [x] **ich:** Markierungen auslesen (`make_db_fixture.py --bookmarks`) und
    mit den erwarteten Offsets vergleichen (Ergebnis in
    `tests/fixtures/README.md`).
  - [x] **du:** Gegenprobe: Ein von uns berechneter XPointer öffnet in KOReader
    die richtige Stelle. Mit Book P (2026-10-24) nur das Kapitel prüfbar, weil
    das Buch etwa eine Bildschirmseite lang ist. Mit Book R (2026-10-24):
    Tolino-Seite beginnt in R1-156 bei Zeichen 253 („spät ruhig Garten.“),
    `push` sendet `…/p[156]/text()[1].253`, KOReader öffnet die Seite, die in
    R1-156 bei Zeichen 44 beginnt und bis R1-160 reicht, also mit der
    Stelle darauf; beide 27 %. Die Seiten beginnen verschieden, weil
    KOReader anders umbricht; die Position selbst stimmt.
  - [x] **ich:** Langes Testbuch **Book R** (`tools/make_long_epub.py`): drei
    Kapitel mit je 200 Absätzen, jeder Absatz mit Marke `R<Kapitel>-<Absatz>`
    (Anleitung in `tests/fixtures/README.md`).
  - [x] **du:** Book R auf den Tolino und in KOReader geladen, auf dem Tolino
    mitten in Kapitel 1 geblättert, `push`, in KOReader Fortschritt geholt
    (Ergebnis oben).
- [x] **K5 KOSync-Client**
  - [x] **ich:** Statt Docker ein kleiner Mock-Server in den Tests
    (`tests/common/mod.rs`): läuft ohne Netz und ohne echtes Konto, auch in
    der CI.
  - [x] **ich:** Anmeldung (`x-auth-user`, `x-auth-key` = MD5 des Passworts),
    Fortschritt lesen (`GET`) und schreiben (`PUT`) (`kosync::Client`).
    HTTPS über `rustls` (ring, eingebaute Stammzertifikate), kein OpenSSL;
    der statische ARM-Build läuft weiter. TLS gegen kosync.eu geprüft
    (Anmeldung mit Dummy-Konto wird mit 401 abgelehnt).
  - [x] **du + ich:** Erster echter `push` gegen kosync.eu mit deinem Konto
    (Book P und Book R vom Tolino, 2026-10-24). Dabei gefunden: kosync.eu
    antwortet ohne gespeicherte Position mit `[]` statt `{}`.
  - [x] **du:** Server: **kosync.eu** (Konto vorhanden). Das Dashboard dort
    zeigt den Lesestand je Hash und kann Testwerte wieder löschen. Das
    Passwort gibst du nur in die lokale Konfiguration ein, nie in den Chat
    oder ins Repo; automatische Tests laufen gegen einen Test-Server ohne
    echtes Konto.

## Programm

- [~] **P1 `status`, `push`, `pull`**
  - [x] **ich:** Konfigurationsdatei `~/.config/nickelinksync/config.ini`
    (Server, Benutzer, Passwort oder MD5-Schlüssel, optional Gerätename und
    -ID; Format im README). Das Passwort steht nicht in der Befehlszeile;
    Warnung, wenn andere die Datei lesen können.
  - [x] **ich:** `status`: Position auf dem Gerät und auf dem Server
    gegenüberstellen, mit Hinweis, welche Seite neuer ist.
  - [x] **ich:** `push`: Position zum Server. Eine neuere Server-Position
    (Server-Zeitstempel nach `DateLastRead`) wird nur mit `--force`
    überschrieben; `--dry-run`, `--book`.
  - [x] **ich:** `pull`: nur anzeigen, was sich ändern würde (Probelauf).
    Die neueste Position gewinnt; rückwärts nur nach Rückfrage (P3).
  - [x] **du:** Passwort in die lokale Konfiguration eingetragen, `push` vom
    Tolino Shine Color, Buch in KOReader mit Sync geöffnet: landet an der
    richtigen Stelle (Book R, siehe K4).
  - [ ] **du:** Ein Durchlauf mit Kobo, Tolino und KOReader am selben Buch.
- [~] **P2 Rückschreiben per USB** (erst Tolino, Kobo im Kobo-Durchgang)
  - [x] **ich:** `pull --write` (`db::Device::write_positions`):
    Sicherungskopie nach `~/.local/share/nickelinksync/backups/`, dann in
    einer Transaktion `ChapterIDBookmarked` und `___PercentRead` setzen;
    `DateLastRead` bleibt, außer es ist neuer als der Server, dann eine
    Sekunde vor dem Server-Zeitstempel (Begründung in `SCOPE.md`). Die
    Tolino-Datenbank ist im WAL-Modus: exklusive Sperre, Checkpoint nach dem
    Commit, danach keine `-wal`/`-shm`/`-journal`-Dateien. Schreibt nur mit
    `.kobo/version`. An einer Kopie der Shine-Color-Datenbank geprüft.
  - [x] **du:** Am Tolino geprüft (2026-10-24, Book R): KOReader-Seite
    beginnt am Ende von R2-134, `pull --write` schreibt
    `chapter2.xhtml#point(/1/4/270/1:269)`, keine Journal-Dateien übrig.
    Nach dem Auswerfen öffnet Nickel die Seite mit dieser Stelle (oben
    R2-133) und speichert beim Schließen den eigenen Seitenanfang
    (`#point(/1/4/268/1:46)`, 57 %). **Nickel übernimmt die Position.**
  - [ ] **ich (Kobo-Durchgang):** dasselbe für den Kobo (braucht K4
    rückwärts für kepub).

- [x] **P3 Rückfrage bei abweichender Position (einstellbar)** · PR #18
  - [x] **ich:** `confirm = never | backwards | always` in der Konfiguration
    (`sync::decide`). „Rückwärts“: Die Position, die überschrieben würde,
    liegt weiter hinten im Buch (Spine-Index, im selben Kapitel die Stelle
    im Text, Prozent nur als Rückfall; seit P2, weil Nickel und KOReader
    den Prozentwert verschieden berechnen), nicht nach Zeitstempel, weil
    Nickel `DateLastRead` schon beim Öffnen setzt. Gilt
    für `push` und `pull`; im Terminal Frage „[d]evice or [s]erver?“, ohne
    Terminal wird nichts überschrieben und das Buch gemeldet; `--force`
    (jetzt auch bei `pull`) überschreibt ohne Frage. Später im Plugin als
    Dialog (O3/O4).
  - [x] **du:** Standardwert festgelegt: `backwards`.
  - [x] **du:** Am Tolino geprüft (2026-10-24, Book R): KOReader bei 45 %
    in Kapitel 2 synchronisiert (10:21:06), Buch auf dem Tolino nur
    geöffnet (`DateLastRead` 10:21:28, Position 27 % in Kapitel 1).
    `status` meldet „push asks first“, `push` fragt, Antwort `s` behält die
    Server-Position; mit `< /dev/null` wird nichts gesendet.

## Plugin

- [ ] **N1 Symbole für Kobo 4.46 (Qt5)**
  - [ ] **du:** Firmware-Update 4.46 für den Clara BW besorgen (wegen `libnickel.so`).
  - [ ] **ich:** Symbole für „Buch geöffnet“, „Buch geschlossen“ und „an
    Position springen“ suchen.
- [ ] **N2 Plugin Qt5**
  - [ ] **ich:** NickelHook-Plugin, das das Programm beim Schließen (`push`)
    und Öffnen (`pull`) aufruft; Paket `KoboRoot.tgz`.
  - [ ] **du:** Test auf dem Clara BW.
- [x] **N3 Symbole für Tolino 5.18 (Qt6)** · Ergebnis in
  [`docs/nickel-5.18-symbols.md`](docs/nickel-5.18-symbols.md)
  - [x] **du:** `libnickel.so` aus dem Kobo-Build 5.18.270971 und die
    Symbolliste des Tolino-Builds 5.18.264769 bereitgestellt.
  - [x] **ich:** Buch geöffnet: `ReadingController::setVolume(Volume
    const&, Bookmark const&)`; Buch verlassen:
    `ReadingView::leavingReadingExperience()` (speichert die Position
    vorher selbst). Signaturen und Rückgabetypen per Disassembly geprüft,
    alle Symbole in der Tolino-Liste.
  - [x] **ich:** Die Datenbank allein reicht im Plugin nicht: Beim Öffnen
    hat Nickel das Buch schon gelesen und schreibt seine Fassung beim
    Blättern zurück. Stattdessen im Öffnen-Hook eine Kopie des `Volume`
    mit neuer Position übergeben (`Content::setChapterIdBookmarked`,
    `setPercentRead`); Rückfall `ReadingView::goToBookmark`. Am Gerät
    noch zu prüfen (N4).
  - [x] **ich (K1-Rest, N4):** Auf dem Gerät die Datenbank mit `mode=ro`
    lesen, nicht `immutable=1` (`--live`): Im WAL-Modus stünde Nickels
    letzte Änderung sonst noch in der `-wal`-Datei und fehlte.
- [x] **N4 Plugin Qt6** · PR #22
  - [x] **ich:** NickelHook-Plugin (`plugin/`, Submodul
    `nicoverbruggen/NickelHook`, Branch `qt6-support`): Öffnen-Hook holt die
    Position (`pull --live --emit`, höchstens etwa 15 s) und setzt sie in
    das Buch, das Nickel öffnet, Verlassen-Hook sendet sie im Hintergrund
    (`push --live`). Nur Tolino, nur mit Programm und `config.ini` in
    `.adds/NickelInkSync/`; Log `NickelInkSync.log` daneben.
  - [x] **ich:** CLI auf dem Gerät: `--live` (`mode=ro`, nie schreiben),
    `--content-id`, `--timeout`, `pull --emit` (K1-Rest).
  - [x] **ich:** Build im Qt-6.5.2-Image
    `ghcr.io/dkuester/nickelstorygraph-toolchain` (nicht `strayrose/nickeltc`:
    das ist Qt 5 und baut kein Plugin für 5.x), Paket `Kobo.tgz`, `just
    plugin` / `just package`. CI-Schritt im Job `check`: Symbole gegen die
    5.18-Liste, ARM, nur Qt 6, Handler exportiert, Artefakt
    `NickelInkSync-tolino`.
  - [x] **du:** Erster Test auf dem Shine Color (2026-10-24, Book R):
    Plugin aktiv, `pull` holt R3-45 (75 %) richtig, Nickel öffnet aber nur
    das Kapitel (R3-1): Beim Öffnen lädt Nickel das Buch noch einmal aus
    der Datenbank (`VolumeManager::getById`) und nimmt die Stelle im Kapitel
    von dort. Der `push` danach (R3-13, hinter dem Server) wurde nach der
    Regel `backwards` korrekt zurückgehalten.
  - [x] **ich:** Zusätzlicher Hook auf `VolumeManager::getById`, nur
    während des Öffnens und nur für dieses Buch: setzt die Position auch
    im neu geladenen Buch (Weg 1, von dir gewählt).
  - [x] **du:** Zweiter Test (2026-10-24): `getById`-Hook greift, Nickel
    öffnet trotzdem am Kapitelanfang (R3-1 statt R3-130). Der `push` beim
    Verlassen scheiterte, weil kein WLAN da war (`failed to lookup
    address`); „Jetzt synchronisieren“ ist die Tolino-Cloud und hatte nur
    nebenbei das WLAN eingeschaltet.
  - [x] **ich:** WLAN selbst einschalten (`WirelessWorkflowManager::
    connectWireless`, wie NickelStorygraph): beim Öffnen höchstens 12 s
    warten, beim Verlassen im Hintergrund bis 45 s. Diagnose im Log: jeder
    `getById` des Buchs mit Aufrufer, wo Nickel 3 s und 10 s nach dem Öffnen
    steht, und die gespeicherte Position in `pull` (Weg A, von dir gewählt).
  - [x] **du:** Dritter Test (2026-10-24): WLAN-Einschalten und `push`
    funktionieren (R3-188 kam an, KOReader zeigt es in der letzten Zeile).
    Öffnen weiter am Kapitelanfang: ein `getById` beim Öffnen, gepatcht,
    Nickel steht trotzdem bei `#point(/1/4/2/1:0)`. Im geladenen `Volume`
    kommt nur das Kapitel an, nie die Stelle darin.
  - [x] **du:** Weg C gewählt: nach dem Öffnen mit Nickels eigener
    Navigation springen (`ReadingView::goToBookmark`); Weg B (Datenbank
    schreiben, während Nickel läuft) nur, wenn C nicht geht.
  - [x] **ich:** Sprung eingebaut: warten, bis das Buch offen ist,
    Nickels Lesezeichen für die aktuelle Stelle nehmen, Start auf die
    geholte Position setzen (`ContentLocation::create`, RMSDK-Format aus
    `#point(`), `goToBookmark`.
  - [x] **du:** Vierter Test (2026-10-24, `confirm = never` zum Testen):
    KOReader-Push R2-63 kam richtig an, Tolino-Push R2-113 auch. Gesprungen
    wurde nicht: `ReadingView::isBookOpened()` blieb 15 s lang falsch,
    obwohl das Buch offen war. (Erster Versuch scheiterte an einer
    angeklebten Zeile in `config.ini`, mein Fehler in der Anleitung.)
  - [x] **ich:** „Offen“ heißt jetzt: Lesesicht aktiv und zweimal hintereinander
    dieselbe Stelle gemeldet.
  - [x] **du:** Fünfter Test (2026-10-24): Sprung gar nicht versucht, weil das
    WLAN beim Öffnen nicht innerhalb von 12 s kam; der WLAN-Dialog (Auswahl
    des Netzes) hat die Lesesicht verlassen und einen `push` ausgelöst.
  - [x] **ich:** Umgebaut (von dir gewünscht): Buch öffnet sofort, im
    Hintergrund WLAN (erst still, nach 15 s mit Nickels WLAN-Dialog, wie von
    dir angeregt), `pull`, Sprung; kein `push`, solange das läuft. Die
    Änderungen am `Volume` und der `getById`-Hook sind entfernt.
  - [x] **du:** Sechster Test (2026-10-24): WLAN und `pull` in einer Sekunde,
    `goToBookmark` meldet „done“, Nickel bleibt aber in Kapitel 2 (Ziel
    Kapitel 1). Danach hat der `push` die neuere KOReader-Position auf dem
    Server mit der Tolino-Position überschrieben.
  - [x] **ich:** Kein `push` mehr, wenn Nickel nach dem Sprung nicht im
    geholten Kapitel steht; `pull --live` schreibt nicht mehr „Dry run“.
  - [x] **du:** Siebter Test (2026-10-24): Sprung innerhalb von Kapitel 2
    (R2-121 nach R2-170): wieder „done“, Nickel bleibt stehen. Weg C ist
    damit gescheitert. Der `push` danach hat R2-170 erneut überschrieben,
    weil die Sperre dagegen erst im neuen Plugin war, das ich dich nicht
    installieren ließ (mein Fehler).
  - [x] **ich:** Weg B (vorher von dir freigegeben, falls C scheitert): vor
    dem Öffnen still WLAN (höchstens 10 s), `pull --live --write --emit`
    schreibt die Position in die Zeile des Buchs, während Nickel läuft
    (SQLite-Sperren, Nickels WAL bleibt, eine Sicherung am Tag mit SQLites
    Backup-Funktion in `.adds/NickelInkSync/backups/`), dazu die
    `Volume`-Kopie für das Kapitel. Kein `push` für ein Buch ohne Abgleich
    beim Öffnen oder außerhalb des geholten Kapitels.
  - [x] **du:** Achter Test (2026-10-24): `isInternetAccessible()` blieb
    10 s falsch, obwohl das WLAN verbunden war; das Buch öffnete
    unverändert, und die Sperre hat den `push` danach richtig verhindert.
  - [x] **ich:** Nickels Internet-Meldung entscheidet nur noch, wie lange
    gewartet wird; `pull` wird immer versucht. Ohne Antwort vom Server kein
    `push`; ohne Neueres auf dem Server ist das Buch abgeglichen und wird
    gesendet (vorher wurde es fälschlich nie gesendet). Braucht `pull` eine
    Antwort (`NICKELINKSYNC_UNANSWERED`), wird ebenfalls nicht gesendet.
  - [x] **du:** Neunter Test (2026-10-24, wieder `confirm = backwards`):
    Der KOReader-Push R3-49 kam nicht auf dem Server an, der Tolino war
    damit neuer und blieb bei R2-121 (richtig nach der Regel). Deine Frage
    nach dem Renderer war berechtigt: Der Tolino rendert mit **TENG**, nicht
    RMSDK; deshalb griffen meine Eingriffe in Nickels Speicher nicht.
  - [x] **du + ich:** Zehnter Test (2026-10-24): Server-Stand vorher geprüft
    (R3-49 von macOS, 16:08 UTC). Beim Öffnen geschrieben und gesichert,
    Nickel öffnet auf der Seite mit R3-49 (oben R3-48). Weitergelesen bis
    R3-58, beim Verlassen gesendet, KOReader zeigt es. **Weg B funktioniert.**
  - [ ] **ich (später, O3/O4):** Rückfrage im Plugin als Dialog, statt eine
    Position, die eine Antwort braucht, liegen zu lassen.

## Optional (später)

- [ ] **O1 Manuelle Zuordnung** von Büchern (kepub ↔ EPUB), Testfall Heat 2.
  **Geht in D1 auf** (CLI mit manueller Buchzuordnung, höchste Priorität).
  Vorbild: BookOrbit, nur als Konzept (AGPL-3.0). Blockiert auch den
  Kindle-Sync (L1): Nichts im KFX führt zur EPUB-Prüfsumme; Kandidaten für
  den Schlüssel sind `content_id` (32 Hex-Ziffern, auch im Dateinamen),
  `title` (= Dateiname beim Senden) und `author`.
- [x] **O2 Normales EPUB auf Kobo 4.x** · **du:** ein normales EPUB auf den Clara
  geladen und geöffnet. Ergebnis: wie auf dem Tolino `#point(...)`,
  Kapitelzeilen sind ToC-Einträge (`SCOPE.md`).
- [ ] **O3 Einstellungsdialog** auf dem Gerät.
- [ ] **O4 Sync-Status** im Buchmenü, plus „Jetzt synchronisieren“.
- [ ] **O5 Geräteübersicht** in `status`: alle Positionen eines Buchs nebeneinander.
- [ ] **O6 Markierungen auf Kobo 5.x/6.x nachtesten** · `[!]` erst, wenn alle
  Tests mit Kobo 4.x durch sind: Das Update auf 5.18+ kostet das einzige
  4.x-Gerät (Clara BW). · **du:** Book P auf dem Kobo mit Firmware 5.18 oder
  6.x markieren. Auf 4.46 zeigt Nickel Markierungen im kepub verschoben und im
  EPUB teils gar nicht an (gespeichert wird richtig); auf dem Tolino mit 5.18
  tritt das nicht auf.

## Langfristig (andere Gerätefamilien)

- [~] **L1 Kindle ↔ KOSync** · Machbarkeit in `SCOPE.md` (Abschnitt Kindle)
  - [x] **du:** Book A, C, P, Q und R per Send-to-Kindle auf den Paperwhite
    11 (5.19.2) geladen, in Book P und Q die Zielwörter markiert, in Book R
    zwei Seiten gelesen (R2-99, R3-85) und die Wörter oben notiert.
  - [x] **ich:** Nur lesend kopiert und ausgewertet: KFX ohne DRM, Position
    `lpr` in der `.yjf` per USB, `eid` + Offset in Codepoints, zeichengenau;
    Abweichungen zu KOReader bei Leerraum am Absatzanfang, Inline-Elementen
    und Kommentaren, CDATA-Text geht bei der Umwandlung verloren. Kein Weg
    vom KFX zur Prüfsumme der EPUB.
  - [x] **ich:** Anonymisierte Kindle-Fixtures für Book P, Q und R
    (`tools/make_kindle_fixture.py`, `tests/fixtures/kindle/`).
  - [x] **du:** Fixtures durchgesehen und freigegeben (2026-10-24).
  - [x] **du:** Festgelegt: Nutzer haben standardmäßig **keinen Jailbreak**.
    Der Funktionsumfang wird getrennt: ohne Jailbreak Desktop-CLI per USB
    (`.yjf`), mit Jailbreak zusätzlich `cc.db` und Sync ohne Kabel
    (Daemon auf dem Gerät). Nichts aus dem Grundumfang darf den Jailbreak
    brauchen.
  - [x] **ich:** KFX-Leser und Übersetzung `eid` + Offset → XPointer
    (`src/ion.rs`, `src/krds.rs`, `src/kfx.rs`, `src/kindle.rs`, eigener
    Ion- und KRDS-Leser): `lpr`, `fpr`, `page.history.record` und
    Markierungen aus `.yjf`/`.yjr`, Text und Positionskarten aus dem KFX;
    jede gespeicherte `pid` passt zu den Karten. Alle Markierungsanfänge in
    Book P und Q und die Book-R-Positionen ergeben KOReaders XPointer;
    nur q5w04 (CDATA) ist nicht exakt (Text weicht ab, gefunden über den
    Text danach).
  - [!] **ich:** Kindle in die CLI: `status --device` für einen gemounteten
    Kindle (KFX + `.sdr` lesen, Position übersetzen), nur lesend.
    Blockiert durch D1 (Buchzuordnung in der CLI): ohne Zuordnung KFX ↔
    EPUB-Prüfsumme kein Sync.
    Der Leser und die Übersetzung liegen fertig in der Bibliothek
    (`kindle::to_xpointer`).
  - [ ] **du:** Prüfen, ob der Kindle eine geänderte `.yjf` übernimmt
    (Flugmodus, damit Amazons Sync nicht dazwischenfunkt).
