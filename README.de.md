<div align="center">

# NickelInkSync

**Gleicht den Lesefortschritt seitengeladener E-Books zwischen Kobo, Tolino und KOReader ab.**

[Scope](SCOPE.md) · [Roadmap](ROADMAP.md) · [English](README.md)

</div>

NickelInkSync sorgt dafür, dass du in einem seitengeladenen Buch auf jedem
E-Reader an derselben Stelle weiterliest. Es nutzt einen
[koreader-sync-server](https://github.com/koreader/koreader-sync-server)
(KOSync-Protokoll), sodass Kobo, Tolino und KOReader denselben Fortschritt
teilen.

> [!IMPORTANT]
> **Stand:** frühe Entwicklung. Mit einem per USB angeschlossenen Kobo oder
> Tolino überträgt `nickelinksync push` die Leseposition jedes
> seitengeladenen Buchs zu einem KOSync-Server, `status` stellt die Position
> auf dem Gerät und auf dem Server gegenüber, und `pull` zeigt, was eine
> neuere Position auf dem Server ändern würde; auf einem Tolino schreibt
> `pull --write` sie aufs Gerät (experimentell, vorher mit
> Sicherungskopie). Den Scope und die Datenbank-Befunde, auf denen er beruht,
> findest du in [SCOPE.md](SCOPE.md), die nächsten Schritte in
> [ROADMAP.md](ROADMAP.md) (beides auf Englisch).

## Benutzung

```sh
nickelinksync status --device /Volumes/KOBOeReader
nickelinksync push --device /Volumes/KOBOeReader
nickelinksync pull --device /Volumes/KOBOeReader
```

| Befehl | Was er tut |
|---|---|
| `status` | Listet die seitengeladenen Bücher mit Prüfsumme, gespeicherter Position, KOReader-XPointer und, falls eingerichtet, der Position auf dem Server. |
| `push` | Überträgt die Position jedes Buchs zum Server. Eine Position, die der Server erst nach dem letzten Lesen auf dem Gerät bekommen hat, wird nie überschrieben; außerdem fragt `push` standardmäßig nach, bevor es den Server an eine frühere Stelle im Buch zurücksetzt (siehe `confirm` unten). `--force` überträgt ohne Rückfrage, `--dry-run` zeigt nur, was gesendet würde. |
| `pull` | Zeigt, für welche Bücher der Server eine neuere Position hat, wie das Gerät sie speichern würde und wo es nachfragen würde. Nur mit `--write` wird sie aufs Gerät geschrieben (bisher nur Tolino, experimentell, siehe unten); `--force` übernimmt die Server-Position ohne Rückfrage. |

Alle drei nehmen `--book <Text>` (nur Bücher, deren Titel oder Dateipfad den
Text enthält) und `--config <Datei>` für eine andere Konfigurationsdatei. Das
Gerät wird nur gelesen, nie beschrieben, außer von `pull --write`.

`pull --write` kopiert zuerst `.kobo/KoboReader.sqlite` nach
`~/.local/share/nickelinksync/backups/KoboReader-<UTC-Zeit>.sqlite` (oder
`$XDG_DATA_HOME/nickelinksync/backups`, oder `$NICKELINKSYNC_BACKUP_DIR`)
und setzt dann die Position aller Bücher in einer Transaktion. Es schreibt
nicht ohne `.kobo/version` (kein eingehängtes Gerät) und nicht, solange
SQLite-Journaldateien neben der Datenbank liegen. Wirf das Gerät aus, bevor
du das Buch öffnest. Rückgängig machen: die Sicherung bei eingehängtem Gerät
zurück nach `.kobo/KoboReader.sqlite` kopieren.

## Konfiguration

`push` und `pull` brauchen eine Konfigurationsdatei, `status` nutzt sie, wenn
es sie gibt. Standardort: `~/.config/nickelinksync/config.ini` (oder
`$XDG_CONFIG_HOME/nickelinksync/config.ini`, oder der Pfad in
`$NICKELINKSYNC_CONFIG`). Das Format steht im [englischen README](README.md#configuration):
`server`, `user`, `password` (oder `key` = MD5 des Passworts), optional
`device`, `device_id` und `confirm`.

`confirm` legt fest, wann `push` und `pull` „Gerät oder Server?“ fragen,
statt die neuere Position gewinnen zu lassen:

| Wert | Fragt |
|---|---|
| `never` | nie: die neuere Position gewinnt |
| `backwards` (Standard) | wenn die Position, die überschrieben würde, weiter hinten im Buch liegt als die neue |
| `always` | immer, wenn sich die beiden Positionen unterscheiden |

„Weiter hinten“ entscheidet die Stelle im Buch (Kapitel, dann Prozent),
nicht die Uhrzeit: Nickel vermerkt ein Buch schon beim Öffnen als gelesen,
ein auf dem E-Reader nur geöffnetes Buch wirkt also neuer als die Seite, die
du in KOReader erreicht hast. Im Terminal antwortest du mit `d` (Gerät) oder
`s` (Server), Enter überspringt das Buch. Ohne Terminal wird nichts
überschrieben und das Buch gemeldet. `--force` überschreibt ohne Rückfrage.

Das Passwort steht nur in dieser Datei, nie in der Befehlszeile. Halte die
Datei privat (`chmod 600`); NickelInkSync warnt, wenn andere sie lesen
können.

## Geplant

| Teil | Was er tut |
|---|---|
| **CLI** (Rust) | Liest die Position von einem per USB verbundenen Kobo oder Tolino, rechnet sie in eine KOReader-Position um und gleicht sie mit einem KOSync-Server ab. Läuft auf dem Rechner und auf dem Gerät. |
| **Plugin** (NickelHook) | Überträgt die Position beim Schließen eines Buchs und holt beim Öffnen eine neuere ab, per WLAN. |

## Geräte und Bücher

- Kobo mit Firmware 4.x sowie Tolino Shine 5, Shine Color und Vision Color mit
  Firmware 5.x
- Seitengeladene kepub- und EPUB-Bücher ohne DRM
- Vorerst muss auf jedem Gerät **dieselbe Datei** liegen. Der Sync-Server
  erkennt Bücher an einer Prüfsumme der Datei, ein kepub auf dem einen und das
  ursprüngliche EPUB auf dem anderen Gerät gelten also als zwei verschiedene
  Bücher. Eine manuelle Zuordnung steht auf der Roadmap.

## Vorgeschichte

In diesem Repository lag früher `inksync`, ein Export von Markierungen und
Lesezeit vom Kobo nach Obsidian. Er wurde mit dem Scope-Wechsel entfernt; die
letzte Version steht in der Git-Historie vor diesem Wechsel.

## Bauen

Benötigt eine stabile [Rust](https://rustup.rs)-Toolchain.

```sh
cargo build --release
```

Das Programm liegt danach unter `target/release/nickelinksync`. HTTPS läuft
über `rustls` mit eingebauten Stammzertifikaten, OpenSSL wird nicht gebraucht,
auch nicht auf dem Gerät.

Für das Gerät (Kobo und Tolino, 32-Bit-ARM) baut man ein statisch gelinktes
Programm mit [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild):

```sh
rustup target add armv7-unknown-linux-musleabihf
pip install cargo-zigbuild   # installiert auch Zig
just build-device
```

Es liegt dann unter `target/armv7-unknown-linux-musleabihf/release/nickelinksync`.
`just check-device` ruft zusätzlich `--version` in einem ARM-Container auf
(braucht Docker). Jeder CI-Lauf lädt dieselbe Datei als Artefakt
`nickelinksync-armv7-musleabihf` hoch.

## Support

| | |
|---|---|
| Fehler und Ideen | [Issues](https://github.com/dkuester/NickelInkSync/issues). Bitte Gerätemodell und Firmware-Version angeben. |
| Mitwirken | Pull-Requests gegen `main`. |

## Lizenz

[MIT](LICENSE)
