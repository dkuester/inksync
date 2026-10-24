#!/usr/bin/env python3
"""Every libnickel symbol the plugin relies on must exist in firmware 5.18.

- `.sym = "..."` (nh_hook): hooked through libnickel's GOT, so libnickel must
  define it (D) or import it (U).
- `.name = "..."` (nh_dlsym): resolved with dlsym, so it must be defined (D).

The reference lists the `_Z` dynamic symbols of `libnickel.so.1.0.0` from
the Tolino build 5.18.264769, one "D name" / "U name" per line, gzipped. It
comes from firmware, so it is not in this repository: CI downloads it from
the owner's NickelTolinoShopRedirect repository at a fixed commit.

Usage: check_plugin_symbols.py <libnickel-5.18.syms.gz> [plugin source]
"""

import gzip
import re
import sys
from pathlib import Path

SOURCE = Path(__file__).resolve().parent.parent / "plugin" / "src" / "nickelinksync.cc"
# `\s*=` right after the field name excludes `.sym_new = "..."`.
HOOK_RE = re.compile(r'\.sym\s*=\s*"([^"]+)"')
DLSYM_RE = re.compile(r'\.name\s*=\s*"(_Z[^"]+)"')


def main() -> int:
    if len(sys.argv) not in (2, 3):
        print(__doc__.strip().splitlines()[-1], file=sys.stderr)
        return 2
    src = Path(sys.argv[2] if len(sys.argv) == 3 else SOURCE).read_text(encoding="utf-8")
    defined, imported = set(), set()
    with gzip.open(sys.argv[1], "rt", encoding="utf-8") as f:
        for line in f:
            kind, _, name = line.strip().partition(" ")
            (defined if kind == "D" else imported).add(name)
    hooks, dlsyms = HOOK_RE.findall(src), DLSYM_RE.findall(src)
    errors = [f"hook symbol not in libnickel 5.18: {s}" for s in hooks if s not in defined | imported]
    errors += [f"dlsym symbol not defined in libnickel 5.18: {s}" for s in dlsyms if s not in defined]
    if not hooks or not dlsyms:
        errors.append("no hooks or dlsym entries found in the plugin source")
    for e in errors:
        print(e, file=sys.stderr)
    if errors:
        return 1
    print(f"{len(hooks)} hooks and {len(dlsyms)} dlsym symbols found in libnickel 5.18")
    return 0


if __name__ == "__main__":
    sys.exit(main())
