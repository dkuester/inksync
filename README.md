<div align="center">

# NickelInkSync

**Sync the reading progress of sideloaded ebooks between Kobo, Tolino and KOReader.**

[Scope](SCOPE.md) · [Roadmap](ROADMAP.md) · [Deutsch](README.de.md)

</div>

NickelInkSync keeps your place in a sideloaded book in step across e-readers.
It talks to a [koreader-sync-server](https://github.com/koreader/koreader-sync-server)
(the KOSync protocol), so Kobo, Tolino and KOReader devices can all share
the same progress.

> [!IMPORTANT]
> **Status:** early development. With a USB-mounted Kobo or Tolino,
> `nickelinksync push` sends the reading position of each sideloaded book
> to a KOSync server, `status` shows the device and server positions side by
> side, and `pull` shows what a newer server position would change; on a
> Tolino, `pull --write` writes it to the device (experimental, with a
> backup first). The scope and the device-database findings it
> rests on are in [SCOPE.md](SCOPE.md), the next steps are in
> [ROADMAP.md](ROADMAP.md).

## Usage

```sh
nickelinksync status --device /Volumes/KOBOeReader
nickelinksync push --device /Volumes/KOBOeReader
nickelinksync pull --device /Volumes/KOBOeReader
```

| Command | What it does |
|---|---|
| `status` | Lists the sideloaded books with hash, stored position, KOReader XPointer and, if configured, the position on the server. |
| `push` | Sends each book's position to the server. A position the server got after the book was last read on the device is never overwritten; by default it also asks before moving the server back to an earlier place in the book (see `confirm` below). `--force` sends without asking, `--dry-run` only shows what would be sent. |
| `pull` | Shows which books have a newer position on the server, how the device would store it and where it would ask first. Only with `--write` is it written to the device (Tolino so far, experimental, see below); `--force` takes the server position without asking. |

All three take `--book <text>` to limit them to books whose title or file
path contains the text, and `--config <file>` for another configuration
file. The device is only read, never written, except by `pull --write`
(and by the plugin, see below).

`pull --write` first copies `.kobo/KoboReader.sqlite` to
`~/.local/share/nickelinksync/backups/KoboReader-<UTC time>.sqlite` (or
`$XDG_DATA_HOME/nickelinksync/backups`, or `$NICKELINKSYNC_BACKUP_DIR`),
then sets the position of all books in one transaction. It refuses to
write without `.kobo/version` (not a mounted device) or while SQLite
journal files lie next to the database. Eject the device before you open
the book. To undo, copy the backup back to `.kobo/KoboReader.sqlite` while
the device is mounted.

## Configuration

`push` and `pull` need a configuration file, `status` uses it if it exists.
Default location: `~/.config/nickelinksync/config.ini` (or
`$XDG_CONFIG_HOME/nickelinksync/config.ini`, or the path in
`$NICKELINKSYNC_CONFIG`).

```ini
# KOSync server and account
server = https://kosync.eu
user = alice
password = your password
# or, instead of the password, its MD5 hash:
# key = 5ebe2294ecd0e0f08eab7690d2a6ee69

# How the device appears on the server (optional)
# device = Tolino Shine Color
# device_id = 0123456789ABCDEF0123456789ABCDEF

# When to ask before overwriting a position: never, backwards (default), always
# confirm = backwards
```

- The password is only read from this file, never from the command line.
  Keep the file private (`chmod 600`); NickelInkSync warns if others can
  read it. Only the MD5 of the password is sent, as the KOSync protocol
  requires.
- Without `device`, each e-reader appears under its model name (for example
  "Tolino Shine Color"), with a `device_id` derived from that name. Set both
  only if one configuration should always report the same device.
- `confirm` decides when `push` and `pull` ask "device or server?" instead
  of letting the newer position win:

  | Value | Asks |
  |---|---|
  | `never` | never: the newer position wins |
  | `backwards` (default) | when the position that would be overwritten lies further on in the book than the new one |
  | `always` | whenever the two positions differ |

  "Further on" is decided by the place in the book (chapter, then the
  place in the chapter's text, the percentage only if that does not
  decide), not by time: Nickel marks a book as read the moment you open
  it, so a book you only opened on the e-reader looks newer than the page
  you reached in KOReader. In a terminal you answer `d` (device) or `s`
  (server), Enter skips the book. Without a terminal nothing is overwritten
  and the book is reported. `--force` overwrites without asking.
- Lines starting with `#` or `;` are comments; unknown keys are an error.

## Plugin for the Tolino (experimental)

On a Tolino with firmware 5.x, a NickelHook plugin syncs without a
computer, over Wi-Fi: when you open a sideloaded book it fetches a newer
position from the server and the book opens there, when you leave the
book it sends yours. It
calls the same `nickelinksync` program on the device and follows the same
`confirm` rule; it cannot ask yet, so a position that needs an answer is
left alone and noted in the log. On a Kobo the plugin stays inactive for
now.

1. Get `NickelInkSync-tolino` from the latest CI run (or `just package`,
   see [Build](#build)). It holds `Kobo.tgz` and a `.adds/NickelInkSync/`
   folder with the program and `config.ini.example`.
2. Connect the Tolino over USB. Copy the `NickelInkSync` folder into the
   hidden `.adds/` folder and save `config.ini.example` there as
   `config.ini` with your account (same format as
   [above](#configuration)).
3. Copy `Kobo.tgz` into the hidden `.kobo/` folder and eject. The Tolino
   installs the plugin and restarts.

The log is `.adds/NickelInkSync/NickelInkSync.log`. The plugin switches
Wi-Fi on when it needs it; there is no need for the Tolino's own
"Sync now" (that is the Tolino cloud). Opening a book waits at most about
10 seconds for Wi-Fi (no dialog) and 15 seconds for the server. To open
at the server's position, the plugin writes it into the book's row in the
device database just before Nickel opens the book, while Nickel runs; it
keeps one backup of the database a day in `.adds/NickelInkSync/backups/`
(the two newest). Without Wi-Fi the book opens unchanged, and leaving it
then sends nothing, so an older Tolino position never overwrites a newer
one on the server. Leaving a book never waits: the push follows when
Wi-Fi is up, after 15 seconds with the Tolino's Wi-Fi dialog.

**Uninstall:** create an empty file `.adds/NickelInkSync/uninstall`, eject
and power the Tolino off and on again (the file is only checked when the
Tolino starts).

## Devices and books

- Kobo on firmware 4.x and Tolino Shine 5, Shine Color and Vision Color on
  firmware 5.x
- Sideloaded kepub and EPUB books without DRM
- For now, put the **same file** on every device. The sync server tells
  books apart by a hash of the file, so a kepub on one device and the original
  EPUB on another count as two different books. Manual mapping between them
  is on the roadmap.

## History

This repository used to hold `inksync`, a Kobo-to-Obsidian export of
highlights and reading time. It was removed when the project changed scope.
The last version is in the Git history before the scope change.

## Build

Needs a stable [Rust](https://rustup.rs) toolchain.

```sh
cargo build --release
```

The binary is `target/release/nickelinksync`. HTTPS uses `rustls` with
built-in root certificates, so no OpenSSL is needed, also not on the device.

For the device (Kobo and Tolino, 32-bit ARM), build a statically linked binary
with [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild):

```sh
rustup target add armv7-unknown-linux-musleabihf
pip install cargo-zigbuild   # also installs Zig
just build-device
```

The binary is `target/armv7-unknown-linux-musleabihf/release/nickelinksync`.
`just check-device` also runs `--version` in an ARM container (needs Docker).

The plugin (`plugin/`, NickelHook as a submodule) is built for Qt 6.5.2 in
the toolchain image `ghcr.io/dkuester/nickelstorygraph-toolchain` (Docker,
amd64): `just plugin` gives `plugin/Kobo.tgz`, `just package` puts it
together with the device binary into `dist/NickelInkSync/`. Clone with
`--recurse-submodules`, or run `git submodule update --init`.
Each CI run uploads the same file as the artifact `nickelinksync-armv7-musleabihf`.

## Support

| | |
|---|---|
| Bugs and ideas | [Issues](https://github.com/dkuester/NickelInkSync/issues). Please include your device model and firmware version. |
| Contributing | Pull requests against `main`. |

## License

[MIT](LICENSE)
