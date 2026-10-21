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
