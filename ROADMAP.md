# Roadmap

NickelInkSync syncs reading progress of sideloaded books between Kobo, Tolino
and KOReader through a KOSync server. Scope and architecture:
[SCOPE.md](SCOPE.md).

Items are grouped by effort vs. value: **quick wins** (ship fast), **high
priority** (highest value, worth the effort), and **low priority (optional)**
(nice-to-have, safe to defer). Effort: S = an afternoon, M = a few days,
L = a week or more, including verification.

## Recently completed

- [x] Scope fixed: Rust core + CLI (desktop and on-device) plus a NickelHook plugin
- [x] Project renamed to NickelInkSync (GitHub); Codeberg stays as a read-only archive
- [x] Legacy Obsidian export (`inksync.py`, `reading_stats.py`) and its CI removed
- [x] Stale `migrate-to-calibre-plugin` branch deleted (still in the Codeberg archive)
- [x] Placeholder CI job `check` (no device databases, local Markdown links) for the `main` ruleset
- [x] Book identity for the first version: same file on every device, KOReader binary hash
- [x] Kobo 4.46 database checked (Clara BW): kepub position format, chapter rows,
      spine index → `DocFragment` mapping
- [x] Tolino 5.18 database checked (Shine 5): plain-EPUB position format
      (`#point(...)`), UUID chapter rows, Adobe point path resolved on two books

## Quick wins

- **License** (S): replace CC BY-NC 4.0 with a software license. Why: CC
  discourages its licenses for code, and BY-NC is not GPL-compatible, which
  blocks reuse of KOReader code and redistribution alongside it. If code from
  BookOrbit's position converter is ever ported, the project has to be AGPL-3.0.
- **Rust skeleton + CI** (S): crate, `clippy`/`rustfmt`, host tests, a
  cross-build job for `arm-kobo-linux-gnueabihf-`. Why: everything below
  builds on it. Keep the job name `check`, which the `main` ruleset requires.

## High priority

- **Read-only device database layer** (S–M): open `KoboReader.sqlite`
  read-only, list sideloaded books (`ContentType = 6`, `file:///mnt/onboard/`),
  read `ChapterIDBookmarked`, `___PercentRead`, `DateLastRead`; resolve
  chapters through `BookID` + `VolumeIndex`, not the chapter `ContentID`
  format. Fixtures: the Clara BW and Shine 5 dumps, trimmed and anonymized.
- **Document hash** (S): KOReader partial MD5 for a book file, with a test
  against a hash that KOReader itself reports for the same file.
- **Position translation, chapter level** (M): `href#kobo.N.M` and
  `href#point(...)` → spine index → `DocFragment[n]`, percentage as fallback.
  Verified on both reference devices.
- **Position translation, precise** (M–L): Kobo span and Adobe point ↔
  full XPointer. Blocker: a KOReader `metadata.epub.lua` for one of the
  reference books, to confirm the DOM path KOReader expects.
- **KOSync client + `push`/`pull`/`status` CLI** (M): auth headers, progress
  GET/PUT, newest-wins conflict rule, dry-run by default for anything that
  writes. Verification needs a reachable koreader-sync-server.
- **USB write-back** (M): write a pulled position into the database only
  while the device is mounted, with a backup copy first. Blocker: confirm that
  Nickel picks the new position up after eject instead of overwriting it.
- **NickelHook plugin, Qt5 (Kobo 4.x)** (L): push on book close, pull on open,
  jump via Nickel's own navigation. Blocker: find the libnickel symbol for
  "open at position" on 4.46; verification is on-device only.
- **NickelHook plugin, Qt6 (Tolino 5.x)** (L): same plugin, Qt6 build and
  `Kobo.tgz` packaging. Blocker: the libnickel symbols of 5.18.

## Low priority (optional)

- **Manual book mapping *(optional)*** (M–L): link several file hashes (kepub
  and source EPUB, or two different editions) to one book so they share
  progress; a manual link wins over the automatic hash match. Translating the
  position between two different files needs matching chapter hrefs or a
  text anchor. Reference: BookOrbit's hash matching and position converter
  (AGPL-3.0, concept only). Test case: *Heat 2* on the two reference devices.
- **Plain EPUB on Kobo 4.x *(optional)*** (M): check whether 4.x stores
  `#point(...)` like 5.18 or uses `adobe_location`. Blocker: a sideloaded
  plain EPUB on the Kobo.
- **Settings dialog on the device *(optional)*** (M): server, user and
  device name in Nickel instead of editing `config.ini`.
- **Sync status in the book menu *(optional)*** (M): "last synced from
  <device>, <time>", plus a manual "sync now" action.
- **Multi-device conflict view on the desktop *(optional)*** (S): `status`
  lists every device's position for a book side by side.
