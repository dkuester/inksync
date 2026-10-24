#!/usr/bin/env python3
"""Build Book R: a long book for checking positions inside a chapter.

Book P and Book Q fit on about one screen, so a position inside their probe
chapter lands on the same page as the chapter start and a reader that only
shows the chapter start looks right. Book R has three chapters of 200
paragraphs each, many pages at any font size. Every paragraph starts with a
marker `R<chapter>-<paragraph>` (e.g. `R2-117`) and repeats it in the middle,
so the page a reader opens at can be told at a glance and compared with the
pushed XPointer. The filler is German text with umlauts, so lines and pages
break as in a real book.

Writes a plain EPUB and a kepub with Kobo spans, like `make_probe_epub.py`:

    tools/make_long_epub.py tests/fixtures/epub

Needs no input; the output is deterministic.
"""

import argparse
import random
from pathlib import Path

from make_probe_epub import kepubify, page, write

CHAPTERS = 3
PARAGRAPHS = 200
WORDS = (
    "der die das und nicht über Brücke Fluss Straße schön grün Größe hören "
    "Mädchen Bäckerei Brötchen früh spät Wälder Häuser Türen öffnen schließen "
    "lesen Buch Seite Kapitel Abend Morgen Licht Schatten Wind Regen Sonne "
    "langsam schnell weit nah alt neu klein groß müde wach ruhig laut leise "
    "Zug Bahnhof Fenster Tisch Stuhl Lampe Garten Wiese Berg Tal See Ufer "
    "gehen kommen bleiben finden suchen sehen denken sagen fragen antworten"
).split()


def paragraph(chapter, number):
    """About forty words, with the marker at the start and in the middle."""
    rng = random.Random(chapter * 1000 + number)
    words = [rng.choice(WORDS) for _ in range(40)]
    marker = f"R{chapter}-{number}"
    words.insert(20, f"({marker})")
    return f"<p>{marker} " + " ".join(words) + ".</p>"


def chapter_body(chapter):
    paragraphs = "\n".join(paragraph(chapter, n) for n in range(1, PARAGRAPHS + 1))
    return f"\n<h1>Kapitel {chapter}</h1>\n{paragraphs}\n"


def chapter_href(chapter):
    return f"text/chapter{chapter}.xhtml"


SPINE = ["text/title.xhtml"] + [chapter_href(c) for c in range(1, CHAPTERS + 1)] + ["text/end.xhtml"]
TOC = [("Titel", "text/title.xhtml")] + [(f"Kapitel {c}", chapter_href(c)) for c in range(1, CHAPTERS + 1)]


def opf(kepub):
    flavor = "kepub" if kepub else "epub"
    items = "\n".join(
        f'    <item id="s{i}" href="{href}" media-type="application/xhtml+xml"/>' for i, href in enumerate(SPINE)
    )
    refs = "\n".join(f'    <itemref idref="s{i}"/>' for i in range(len(SPINE)))
    return (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">\n'
        '  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">\n'
        f'    <dc:identifier id="uid">urn:nickelinksync:book-r-{flavor}</dc:identifier>\n'
        "    <dc:title>Book R</dc:title>\n    <dc:creator>Author R</dc:creator>\n"
        "    <dc:language>de</dc:language>\n"
        '    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>\n'
        "  </metadata>\n  <manifest>\n"
        '    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>\n'
        '    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>\n'
        f"{items}\n  </manifest>\n"
        f'  <spine toc="ncx">\n{refs}\n  </spine>\n</package>\n'
    )


def ncx():
    points = "\n".join(
        f'    <navPoint id="n{i}" playOrder="{i + 1}"><navLabel><text>{label}</text></navLabel>'
        f'<content src="{src}"/></navPoint>'
        for i, (label, src) in enumerate(TOC)
    )
    return (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">\n'
        '  <head><meta name="dtb:uid" content="book-r"/></head>\n'
        "  <docTitle><text>Book R</text></docTitle>\n"
        f"  <navMap>\n{points}\n  </navMap>\n</ncx>\n"
    )


def nav():
    links = "".join(f'<li><a href="{src}">{label}</a></li>' for label, src in TOC)
    return page("Book R", f'\n<nav xmlns:epub="http://www.idpf.org/2007/ops" epub:type="toc"><ol>{links}</ol></nav>\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("out_dir", nargs="?", default="tests/fixtures/epub")
    out_dir = Path(parser.parse_args().out_dir)
    container = (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">\n'
        '  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>\n'
        "</container>\n"
    )
    pages = {
        "text/title.xhtml": page(
            "Book R", "\n<h1>Book R</h1>\n<p>Author R. Ein langes Testbuch für NickelInkSync.</p>\n"
        ),
        "text/end.xhtml": page("Ende", "\n<p>Ende von Book R.</p>\n"),
    }
    for c in range(1, CHAPTERS + 1):
        pages[chapter_href(c)] = page(f"Kapitel {c}", chapter_body(c))
    epub = {f"OEBPS/{name}": data for name, data in pages.items()}
    kepub = {f"OEBPS/{name}": kepubify(data) for name, data in pages.items()}
    for files, kepub_flag in ((epub, False), (kepub, True)):
        files.update({
            "META-INF/container.xml": container,
            "OEBPS/content.opf": opf(kepub_flag),
            "OEBPS/toc.ncx": ncx(),
            "OEBPS/nav.xhtml": nav(),
        })
    write(out_dir / "book-r.epub", epub)
    write(out_dir / "book-r.kepub.epub", kepub)
    print(f"{out_dir}/book-r.epub, {out_dir}/book-r.kepub.epub: {CHAPTERS} chapters of {PARAGRAPHS} paragraphs,"
          f" chapter c at spine index c (DocFragment[c + 1])")


if __name__ == "__main__":
    main()
