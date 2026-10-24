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
> **Status:** early development. `nickelinksync status --device <mount point>`
> lists the sideloaded books of a USB-mounted Kobo or Tolino with their
> KOReader hash and stored position; syncing is not implemented yet. The
> scope and the device-database findings it rests on are in
> [SCOPE.md](SCOPE.md), the next steps are in [ROADMAP.md](ROADMAP.md).

## Planned

| Part | What it does |
|---|---|
| **CLI** (Rust) | Reads the position from a USB-mounted Kobo or Tolino, translates it to a KOReader position and syncs it with a KOSync server. Runs on the desktop and on the device. |
| **Plugin** (NickelHook) | Pushes your position when you close a book and pulls a newer one when you open it, over Wi-Fi. |

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

The binary is `target/release/nickelinksync`. It only prints its version and
help so far.

For the device (Kobo and Tolino, 32-bit ARM), build a statically linked binary
with [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild):

```sh
rustup target add armv7-unknown-linux-musleabihf
pip install cargo-zigbuild   # also installs Zig
just build-device
```

The binary is `target/armv7-unknown-linux-musleabihf/release/nickelinksync`.
`just check-device` also runs `--version` in an ARM container (needs Docker).
Each CI run uploads the same file as the artifact `nickelinksync-armv7-musleabihf`.

## Support

| | |
|---|---|
| Bugs and ideas | [Issues](https://github.com/dkuester/NickelInkSync/issues). Please include your device model and firmware version. |
| Contributing | Pull requests against `main`. |

## License

[MIT](LICENSE)
