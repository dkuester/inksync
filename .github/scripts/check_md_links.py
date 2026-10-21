"""Check local links in tracked Markdown files: target files must exist and
#anchors must match a heading (GitHub slug rules). External links are skipped."""

import re
import subprocess
import sys
from pathlib import Path

LINK = re.compile(r"(?<!!)\[[^\]]*\]\(([^)\s]+)\)")
HEADING = re.compile(r"^#{1,6}\s+(.+?)\s*#*\s*$")
FENCE = re.compile(r"^\s*(```|~~~)")


def slug(heading):
    text = re.sub(r"[`*_]|\[([^\]]*)\]\([^)]*\)", r"\1", heading).strip().lower()
    return re.sub(r"[^\w\- ]", "", text).replace(" ", "-")


def scan(path):
    anchors, links, in_fence = set(), [], False
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if FENCE.match(line):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        if m := HEADING.match(line):
            anchors.add(slug(m.group(1)))
        links += [(lineno, target) for target in LINK.findall(line)]
    return anchors, links


def main():
    files = subprocess.run(["git", "ls-files", "*.md"], capture_output=True, text=True, check=True).stdout.split()
    scanned = {Path(f): scan(Path(f)) for f in files}
    errors = []
    for path, (_, links) in scanned.items():
        for lineno, target in links:
            if re.match(r"^[a-z][a-z0-9+.-]*:", target):
                continue
            file_part, _, anchor = target.partition("#")
            dest = (path.parent / file_part).resolve() if file_part else path.resolve()
            if not dest.exists():
                errors.append(f"{path}:{lineno}: missing file {target}")
                continue
            if anchor and dest.suffix == ".md":
                anchors = scanned.get(dest.relative_to(Path.cwd().resolve()), (set(),))[0]
                if anchor not in anchors:
                    errors.append(f"{path}:{lineno}: missing anchor {target}")
    print("\n".join(errors) or f"{len(files)} Markdown files, all local links resolve")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
