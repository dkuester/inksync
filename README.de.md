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
> **Stand:** Planung, noch nichts umgesetzt. Den Scope und die
> Datenbank-Befunde, auf denen er beruht, findest du in [SCOPE.md](SCOPE.md),
> die nächsten Schritte in [ROADMAP.md](ROADMAP.md) (beides auf Englisch).

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

Das Programm liegt danach unter `target/release/nickelinksync`. Bisher gibt es
nur Version und Hilfe aus.

## Support

| | |
|---|---|
| Fehler und Ideen | [Issues](https://github.com/dkuester/NickelInkSync/issues). Bitte Gerätemodell und Firmware-Version angeben. |
| Mitwirken | Pull-Requests gegen `main`. |

## Lizenz

[MIT](LICENSE)
