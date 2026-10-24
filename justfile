# Device build: a statically linked musl binary for 32-bit ARM with hard
# float. It does not depend on the device's glibc, so the same file runs on
# Kobo firmware 4.x and Tolino firmware 5.x. Needs `cargo-zigbuild` and Zig
# (`pip install cargo-zigbuild` brings both) and the Rust target
# (`rustup target add armv7-unknown-linux-musleabihf`).

target := "armv7-unknown-linux-musleabihf"
device_bin := "target/" + target + "/release/nickelinksync"

default:
  @{{ just_executable() }} --list --justfile {{ justfile() }}

# Host checks, as in CI
check:
  cargo fmt --check
  cargo clippy --all-targets -- -D warnings
  cargo test
  python3 .github/scripts/check_md_links.py

# Build the device binary
build-device:
  cargo zigbuild --release --target {{ target }}
  @echo {{ device_bin }}

# Build the device binary and run `--version` in an ARMv7 container (Docker, QEMU)
check-device: build-device
  file {{ device_bin }}
  docker run --rm --platform linux/arm/v7 -v "$PWD/{{ device_bin }}:/nickelinksync:ro" busybox /nickelinksync --version

# Qt 6.5.2 cross toolchain (arm-kobo-linux-gnueabihf) for the plugin on
# firmware 5.x; amd64 only. The Qt 5 NickelTC image cannot build it.
toolchain := "ghcr.io/dkuester/nickelstorygraph-toolchain:latest"

# Build the plugin for firmware 5.x (plugin/Kobo.tgz), needs Docker
plugin:
  git submodule update --init plugin/NickelHook
  docker run --rm --platform linux/amd64 --volume="$PWD:$PWD" --workdir="$PWD" {{ toolchain }} 'make -C plugin && make -C plugin kobo'
  tar -tzvf plugin/Kobo.tgz

# Everything for the Tolino in dist/NickelInkSync: Kobo.tgz for .kobo/ and
# the folder for .adds/
package: build-device plugin
  rm -rf dist/NickelInkSync
  mkdir -p dist/NickelInkSync/.adds/NickelInkSync
  cp plugin/Kobo.tgz dist/NickelInkSync/Kobo.tgz
  cp {{ device_bin }} dist/NickelInkSync/.adds/NickelInkSync/nickelinksync
  chmod 755 dist/NickelInkSync/.adds/NickelInkSync/nickelinksync
  cp plugin/config.ini.example dist/NickelInkSync/.adds/NickelInkSync/config.ini.example
  find dist/NickelInkSync -type f | sort
