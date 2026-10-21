#!/usr/bin/env python3
"""Build "Book P", a probe book for positions inside a paragraph.

Every word is unique (`p<paragraph>w<word>`), so the same word can be found
and highlighted on a Kobo, a Tolino and in KOReader. The probe chapter mixes
in the cases that may make character offsets differ between renderers: a
no-break space, an entity, a soft hyphen, an inline element, a line break
with indentation inside a text node, and a comment.

Writes two files with the same text: a plain EPUB and a kepub with Kobo
spans (`span.koboSpan#kobo.N.M`, inside `div#book-columns/div#book-inner`).
Then prints, for each target word, where each format should put its start:
the Adobe point (plain EPUB), the Kobo span and offset (kepub), and a naive
KOReader XPointer counted over the source DOM. The device and KOReader data
confirm or refute these.

    tools/make_probe_epub.py tests/fixtures/epub

Needs no input; the output is deterministic.
"""

import re
import sys
import uuid
import xml.dom.minidom as minidom
import zipfile
from pathlib import Path

XHTML_NS = "http://www.w3.org/1999/xhtml"
ZIP_TIME = (2026, 1, 1, 0, 0, 0)
SPINE = ["text/title.xhtml", "text/probe.xhtml", "text/end.xhtml"]
PROBE = 1  # spine index of the probe chapter

# The probe chapter body, pretty-printed on purpose: the whitespace between
# the block elements are text nodes that the Adobe point counts.
PROBE_BODY = """
<h1 id="probe">p0w01 p0w02</h1>
<p>p1w01 p1w02 p1w03 p1w04 p1w05 p1w06 p1w07 p1w08 p1w09 p1w10 p1w11 p1w12.</p>
<p>p2w01 p2w02&#160;p2w03&#160;p2w04 p2w05 p2w06.</p>
<p>p3w01 &amp; p3w02 p3w03 p3w04.</p>
<p>p4w01 p4&#173;w02 p4w03 p4w04.</p>
<p>p5w01 <em>p5w02 p5w03</em> p5w04 p5w05.</p>
<p>p6w01
    p6w02 p6w03 p6w04.</p>
<p>p7w01 <!-- comment --> p7w02 p7w03 p7w04.</p>
"""
# (word, what precedes it)
TARGETS = [
    ("p1w08", "plain text, mid-paragraph"),
    ("p2w04", "two no-break spaces"),
    ("p3w03", "the entity `&amp;`"),
    ("p4w03", "a word with a soft hyphen"),
    ("p5w03", "inside `<em>`, second word"),
    ("p5w04", "text after `</em>`"),
    ("p6w03", "a line break and indentation"),
    ("p7w03", "a comment"),
]


def page(title, body):
    return (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        f'<html xmlns="{XHTML_NS}" xml:lang="en">\n'
        f"<head>\n<title>{title}</title>\n</head>\n"
        f"<body>{body}</body>\n</html>\n"
    )


def kepubify(xhtml):
    """Wrap the body in Kobo's divs and every text run of a block in a Kobo
    span, numbered kobo.<block>.<run> like kepub converters do."""
    doc = minidom.parseString(xhtml.encode("utf-8"))
    body = doc.getElementsByTagName("body")[0]
    columns, inner = doc.createElement("div"), doc.createElement("div")
    columns.setAttribute("id", "book-columns")
    inner.setAttribute("id", "book-inner")
    for child in list(body.childNodes):
        inner.appendChild(child)
    columns.appendChild(inner)
    body.appendChild(columns)
    blocks = [e for e in inner.childNodes if e.nodeType == e.ELEMENT_NODE]
    for number, block in enumerate(blocks, 1):
        runs = []
        stack = [block]
        while stack:
            node = stack.pop()
            for child in node.childNodes:
                if child.nodeType == child.TEXT_NODE and child.data.strip():
                    runs.append(child)
            stack.extend(reversed([c for c in node.childNodes if c.nodeType == c.ELEMENT_NODE]))
        runs.sort(key=lambda n: document_order(doc).index(n))
        for index, text in enumerate(runs, 1):
            span = doc.createElement("span")
            span.setAttribute("class", "koboSpan")
            span.setAttribute("id", f"kobo.{number}.{index}")
            text.parentNode.replaceChild(span, text)
            span.appendChild(text)
    return doc.toxml(encoding="utf-8").decode("utf-8") + "\n"


def document_order(doc):
    nodes, stack = [], [doc.documentElement]
    while stack:
        node = stack.pop()
        nodes.append(node)
        stack.extend(reversed(node.childNodes))
    return nodes


def find_word(doc, word):
    """The text node that holds `word` and the offset of the word in it."""
    hits = [(n, n.data.index(word)) for n in document_order(doc) if n.nodeType == n.TEXT_NODE and word in n.data]
    if len(hits) != 1:
        sys.exit(f"{word} found {len(hits)} times")
    return hits[0]


def adobe_point(node, offset):
    steps = []
    while node.parentNode.nodeType != node.DOCUMENT_NODE:
        steps.append(node.parentNode.childNodes.index(node) + 1)
        node = node.parentNode
    return "point(/1/" + "/".join(str(s) for s in reversed(steps)) + f":{offset})"


def naive_xpointer(node, offset):
    """Element steps by tag index, text() index among text siblings, all
    counted over the source DOM. A hypothesis, not KOReader's algorithm."""
    text_siblings = [c for c in node.parentNode.childNodes if c.nodeType == c.TEXT_NODE]
    steps = [f"text()[{text_siblings.index(node) + 1}].{offset}"]
    node = node.parentNode
    while node.tagName != "body":
        same = [c for c in node.parentNode.childNodes if c.nodeType == c.ELEMENT_NODE and c.tagName == node.tagName]
        steps.append(f"{node.tagName}[{same.index(node) + 1}]")
        node = node.parentNode
    return f"/body[1]/DocFragment[{PROBE + 1}]/body[1]/" + "/".join(reversed(steps))


def opf(kepub):
    book_id = uuid.uuid5(uuid.NAMESPACE_URL, f"nickelinksync-probe-book-p-{'kepub' if kepub else 'epub'}")
    items = "\n".join(
        f'    <item id="s{i}" href="{href}" media-type="application/xhtml+xml"/>' for i, href in enumerate(SPINE)
    )
    refs = "\n".join(f'    <itemref idref="s{i}"/>' for i in range(len(SPINE)))
    return (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">\n'
        '  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">\n'
        f'    <dc:identifier id="uid">urn:uuid:{book_id}</dc:identifier>\n'
        "    <dc:title>Book P</dc:title>\n    <dc:creator>Author P</dc:creator>\n"
        "    <dc:language>en</dc:language>\n"
        '    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>\n'
        "  </metadata>\n  <manifest>\n"
        '    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>\n'
        '    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>\n'
        f"{items}\n  </manifest>\n"
        f'  <spine toc="ncx">\n{refs}\n  </spine>\n</package>\n'
    )


TOC = [("Title", "text/title.xhtml"), ("Probe", "text/probe.xhtml"), ("End", "text/end.xhtml")]


def ncx():
    points = "\n".join(
        f'    <navPoint id="n{i}" playOrder="{i + 1}"><navLabel><text>{label}</text></navLabel>'
        f'<content src="{src}"/></navPoint>'
        for i, (label, src) in enumerate(TOC)
    )
    return (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">\n'
        '  <head><meta name="dtb:uid" content="book-p"/></head>\n'
        "  <docTitle><text>Book P</text></docTitle>\n"
        f"  <navMap>\n{points}\n  </navMap>\n</ncx>\n"
    )


def nav():
    links = "".join(f'<li><a href="{src}">{label}</a></li>' for label, src in TOC)
    return page("Book P", f'\n<nav xmlns:epub="http://www.idpf.org/2007/ops" epub:type="toc"><ol>{links}</ol></nav>\n')


def write(path, files):
    with zipfile.ZipFile(path, "w") as out:
        out.writestr(zipfile.ZipInfo("mimetype", ZIP_TIME), "application/epub+zip", zipfile.ZIP_STORED)
        for name in sorted(files):
            info = zipfile.ZipInfo(name, ZIP_TIME)
            info.external_attr = 0o644 << 16
            out.writestr(info, files[name], zipfile.ZIP_DEFLATED, compresslevel=9)


def main():
    out_dir = Path(sys.argv[1] if len(sys.argv) > 1 else "tests/fixtures/epub")
    container = (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">\n'
        '  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>\n'
        "</container>\n"
    )
    pages = {
        "text/title.xhtml": page("Book P", "\n<h1>Book P</h1>\n<p>Author P. A probe book for NickelInkSync.</p>\n"),
        "text/probe.xhtml": page("Probe", PROBE_BODY),
        "text/end.xhtml": page("End", "\n<p>End of Book P.</p>\n"),
    }
    epub = {f"OEBPS/{name}": data for name, data in pages.items()}
    kepub = {f"OEBPS/{name}": kepubify(data) for name, data in pages.items()}
    for files, kepub_flag in ((epub, False), (kepub, True)):
        files.update({
            "META-INF/container.xml": container,
            "OEBPS/content.opf": opf(kepub_flag),
            "OEBPS/toc.ncx": ncx(),
            "OEBPS/nav.xhtml": nav(),
        })
    write(out_dir / "book-p.epub", epub)
    write(out_dir / "book-p.kepub.epub", kepub)

    plain = minidom.parseString(epub["OEBPS/text/probe.xhtml"].encode("utf-8"))
    spans = minidom.parseString(kepub["OEBPS/text/probe.xhtml"].encode("utf-8"))
    print(f"{out_dir}/book-p.epub, {out_dir}/book-p.kepub.epub: probe chapter OEBPS/{SPINE[PROBE]}"
          f" (spine index {PROBE})\n")
    print("| Word | After | EPUB: Adobe point | kepub: span, offset | KOReader EPUB (naive) | KOReader kepub (naive) |")
    print("|---|---|---|---|---|---|")
    for word, after in TARGETS:
        node, offset = find_word(plain, word)
        knode, koffset = find_word(spans, word)
        span = knode.parentNode.getAttribute("id")
        print(f"| `{word}` | {after} | `{adobe_point(node, offset)}` "
              f"| `{span}`, {koffset} | `{naive_xpointer(node, offset)}` | `{naive_xpointer(knode, koffset)}` |")
    ids = re.findall(r'id="(kobo\.[0-9.]+)"', kepub["OEBPS/text/probe.xhtml"])
    if len(ids) != len(set(ids)):
        sys.exit("duplicate Kobo span IDs")


if __name__ == "__main__":
    main()
