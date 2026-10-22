#!/usr/bin/env python3
"""Build a probe book for positions inside a paragraph: Book P or Book Q.

Every word is unique (`p<paragraph>w<word>`, `q…` in Book Q), so the same
word can be found and highlighted on a Kobo, a Tolino and in KOReader. The
probe chapter puts in front of each target word a case that may make
character offsets differ between renderers. Book P: a no-break space, an
entity, a soft hyphen, an inline element, a line break with indentation and
a comment. Book Q: umlauts, three- and four-byte characters, typographic
quotes, CDATA, leading and repeated spaces, `&lt;` with a tab, and a
combining accent.

Writes two files with the same text: a plain EPUB and a kepub with Kobo
spans (`span.koboSpan#kobo.N.M`, inside `div#book-columns/div#book-inner`).
Then prints, for each target word, where each format should put its start,
by the rules found with Book P (see SCOPE.md): the Adobe point in UTF-8
bytes, the Kobo span offset in characters, and the KOReader XPointer in
characters after collapsing whitespace runs to one space. The device and
KOReader data confirm or refute these.

    tools/make_probe_epub.py tests/fixtures/epub [--book q]

Needs no input; the output is deterministic.
"""

import argparse
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

# The probe chapter bodies, pretty-printed on purpose: the whitespace between
# the block elements are text nodes that the Adobe point counts.
PROBE_BODY_P = """
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
TARGETS_P = [
    ("p1w08", "plain text, mid-paragraph"),
    ("p2w04", "two no-break spaces"),
    ("p3w03", "the entity `&amp;`"),
    ("p4w03", "a word with a soft hyphen"),
    ("p5w03", "inside `<em>`, second word"),
    ("p5w04", "text after `</em>`"),
    ("p6w03", "a line break and indentation"),
    ("p7w03", "a comment"),
]

PROBE_BODY_Q = """
<h1 id="probe">q0w01 q0w02</h1>
<p>q1w01 äöü q1w02 Größe q1w03 q1w04.</p>
<p>q2w01 – q2w02 … q2w03 q2w04.</p>
<p>q3w01 „q3w02“ q3w03 q3w04.</p>
<p>q4w01 𝄞 q4w02 q4w03.</p>
<p>q5w01 <![CDATA[q5w02 <q5w03>]]> q5w04 q5w05.</p>
<p>   q6w01  q6w02   q6w03 q6w04.</p>
<p>q7w01 &lt;q7w02&gt;	q7w03 q7w04.</p>
<p>q8w01 Café q8w02 q8w03.</p>
"""
TARGETS_Q = [
    ("q1w03", "umlauts and ß (two bytes each)"),
    ("q2w03", "`–` and `…` (three bytes each)"),
    ("q3w03", "typographic quotes `„` `“` (three bytes each)"),
    ("q4w02", "a four-byte character (`𝄞`, two UTF-16 units)"),
    ("q5w04", "a CDATA section"),
    ("q6w03", "leading and repeated spaces"),
    ("q7w03", "`&lt;`, `&gt;` and a tab"),
    ("q8w02", "`e` with a combining accent"),
]
BOOKS = {"p": (PROBE_BODY_P, TARGETS_P), "q": (PROBE_BODY_Q, TARGETS_Q)}


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
    """Indices over all child nodes; offset in UTF-8 bytes."""
    data = node.data
    steps = []
    while node.parentNode.nodeType != node.DOCUMENT_NODE:
        steps.append(node.parentNode.childNodes.index(node) + 1)
        node = node.parentNode
    return "point(/1/" + "/".join(str(s) for s in reversed(steps)) + f":{len(data[:offset].encode('utf-8'))})"


def naive_xpointer(node, offset):
    """Element steps by tag index, text() index among text siblings, the
    offset in characters after collapsing whitespace runs (not no-break
    spaces) to one space. A hypothesis, not KOReader's algorithm."""
    text_siblings = [c for c in node.parentNode.childNodes if c.nodeType == c.TEXT_NODE]
    collapsed = len(re.sub(r"[ \t\r\n]+", " ", node.data[:offset]))
    steps = [f"text()[{text_siblings.index(node) + 1}].{collapsed}"]
    node = node.parentNode
    while node.tagName != "body":
        same = [c for c in node.parentNode.childNodes if c.nodeType == c.ELEMENT_NODE and c.tagName == node.tagName]
        steps.append(f"{node.tagName}[{same.index(node) + 1}]")
        node = node.parentNode
    return f"/body[1]/DocFragment[{PROBE + 1}]/body[1]/" + "/".join(reversed(steps))


def opf(letter, kepub):
    book_id = uuid.uuid5(uuid.NAMESPACE_URL, f"nickelinksync-probe-book-{letter}-{'kepub' if kepub else 'epub'}")
    items = "\n".join(
        f'    <item id="s{i}" href="{href}" media-type="application/xhtml+xml"/>' for i, href in enumerate(SPINE)
    )
    refs = "\n".join(f'    <itemref idref="s{i}"/>' for i in range(len(SPINE)))
    return (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">\n'
        '  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">\n'
        f'    <dc:identifier id="uid">urn:uuid:{book_id}</dc:identifier>\n'
        f"    <dc:title>Book {letter.upper()}</dc:title>\n    <dc:creator>Author {letter.upper()}</dc:creator>\n"
        "    <dc:language>en</dc:language>\n"
        '    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>\n'
        "  </metadata>\n  <manifest>\n"
        '    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>\n'
        '    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>\n'
        f"{items}\n  </manifest>\n"
        f'  <spine toc="ncx">\n{refs}\n  </spine>\n</package>\n'
    )


TOC = [("Title", "text/title.xhtml"), ("Probe", "text/probe.xhtml"), ("End", "text/end.xhtml")]


def ncx(letter):
    points = "\n".join(
        f'    <navPoint id="n{i}" playOrder="{i + 1}"><navLabel><text>{label}</text></navLabel>'
        f'<content src="{src}"/></navPoint>'
        for i, (label, src) in enumerate(TOC)
    )
    return (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">\n'
        f'  <head><meta name="dtb:uid" content="book-{letter}"/></head>\n'
        f"  <docTitle><text>Book {letter.upper()}</text></docTitle>\n"
        f"  <navMap>\n{points}\n  </navMap>\n</ncx>\n"
    )


def nav(letter):
    links = "".join(f'<li><a href="{src}">{label}</a></li>' for label, src in TOC)
    return page(f"Book {letter.upper()}", f'\n<nav xmlns:epub="http://www.idpf.org/2007/ops" epub:type="toc"><ol>{links}</ol></nav>\n')


def write(path, files):
    with zipfile.ZipFile(path, "w") as out:
        out.writestr(zipfile.ZipInfo("mimetype", ZIP_TIME), "application/epub+zip", zipfile.ZIP_STORED)
        for name in sorted(files):
            info = zipfile.ZipInfo(name, ZIP_TIME)
            info.external_attr = 0o644 << 16
            out.writestr(info, files[name], zipfile.ZIP_DEFLATED, compresslevel=9)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("out_dir", nargs="?", default="tests/fixtures/epub")
    parser.add_argument("--book", choices=sorted(BOOKS), default="p")
    args = parser.parse_args()
    out_dir, letter = Path(args.out_dir), args.book
    body, targets = BOOKS[letter]
    title = f"Book {letter.upper()}"
    container = (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">\n'
        '  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>\n'
        "</container>\n"
    )
    pages = {
        "text/title.xhtml": page(
            title, f"\n<h1>{title}</h1>\n<p>Author {letter.upper()}. A probe book for NickelInkSync.</p>\n"
        ),
        "text/probe.xhtml": page("Probe", body),
        "text/end.xhtml": page("End", f"\n<p>End of {title}.</p>\n"),
    }
    epub = {f"OEBPS/{name}": data for name, data in pages.items()}
    kepub = {f"OEBPS/{name}": kepubify(data) for name, data in pages.items()}
    for files, kepub_flag in ((epub, False), (kepub, True)):
        files.update({
            "META-INF/container.xml": container,
            "OEBPS/content.opf": opf(letter, kepub_flag),
            "OEBPS/toc.ncx": ncx(letter),
            "OEBPS/nav.xhtml": nav(letter),
        })
    write(out_dir / f"book-{letter}.epub", epub)
    write(out_dir / f"book-{letter}.kepub.epub", kepub)

    plain = minidom.parseString(epub["OEBPS/text/probe.xhtml"].encode("utf-8"))
    spans = minidom.parseString(kepub["OEBPS/text/probe.xhtml"].encode("utf-8"))
    print(f"{out_dir}/book-{letter}.epub, {out_dir}/book-{letter}.kepub.epub: probe chapter OEBPS/{SPINE[PROBE]}"
          f" (spine index {PROBE})\n")
    print("| Word | After | EPUB: Adobe point | kepub: span, offset | KOReader EPUB | KOReader kepub |")
    print("|---|---|---|---|---|---|")
    for word, after in targets:
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
