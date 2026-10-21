#!/usr/bin/env python3
"""Build a small placeholder EPUB with the structure of a real book.

The real books are copyrighted and stay out of the repository. The fixture
keeps what the position code (K3/K4) depends on and nothing else:

- the OPF path and the spine: same number of items, same ids (long digit runs
  zeroed), same hrefs, same spine attributes;
- the table of contents (NCX, and the EPUB 3 nav document if there is one)
  with the same nesting and targets, labels replaced;
- for the chapters named with `--keep`, the complete node tree: every element
  with its `id` and `class`, every whitespace text node unchanged, and every
  other character of text replaced by `x`, so node paths and character
  offsets resolve exactly as in the original.

All other spine items become one-line placeholder pages. Metadata becomes
`Book <X>` / `Author <X>`. Images, fonts and style sheets are dropped.

    tools/make_epub_fixture.py SOURCE.epub OUTPUT.epub --name a \\
        --keep OEBPS/xhtml/xhtml-0-81.xhtml \\
        --position 'OEBPS/xhtml/xhtml-0-81.xhtml#kobo.1.172'

Each `--position` (a `ChapterIDBookmarked` value with the OPF directory in
front) is resolved in the source and in the output and must hit the same kind
of node with the same text length. At the end the output is checked for
leftovers of the original text and metadata, and the script fails if it
finds any.
"""

import argparse
import posixpath
import re
import sys
import uuid
import xml.dom.minidom as minidom
import zipfile

OPF_NS = "http://www.idpf.org/2007/opf"
DC_NS = "http://purl.org/dc/elements/1.1/"
XHTML_NS = "http://www.w3.org/1999/xhtml"
CONTAINER_NS = "urn:oasis:names:tc:opendocument:xmlns:container"
LONG_DIGITS = re.compile(r"\d{10,}")  # ISBNs, order and watermark numbers
KEEP_ATTRS = {
    "id", "class", "xmlns", "xml:lang", "lang", "dir", "epub:type", "type", "rel",
    "http-equiv", "playOrder", "version", "name", "colspan", "rowspan", "style",
}
ZIP_TIME = (2026, 1, 1, 0, 0, 0)
VOID = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"}


def zero_digits(text):
    return LONG_DIGITS.sub(lambda m: "0" * len(m[0]), text)


def filler(text):
    """Replace every character except whitespace, so lengths and line breaks stay."""
    return re.sub(r"\S", "x", text)


def skeletonize(node):
    for child in list(node.childNodes):
        if child.nodeType in (child.TEXT_NODE, child.CDATA_SECTION_NODE, child.COMMENT_NODE):
            child.data = filler(child.data)
        elif child.nodeType == child.ELEMENT_NODE:
            for name in list(child.attributes.keys()):
                value = child.getAttribute(name)
                if name in ("href", "src"):
                    # Keep links inside the book, drop links to the outside.
                    child.setAttribute(name, zero_digits(value) if ":" not in value else "#")
                elif name == "content":
                    child.setAttribute(name, filler(value) if child.getAttribute("name") == "dtb:uid" else zero_digits(value))
                elif name in ("alt", "title"):
                    child.setAttribute(name, filler(value))
                elif name in KEEP_ATTRS or name.startswith("xmlns:"):
                    child.setAttribute(name, zero_digits(value))
                else:
                    child.removeAttribute(name)
            if not child.childNodes and child.namespaceURI == XHTML_NS and child.tagName not in VOID:
                # An empty text node makes minidom write <p></p>, not <p/>,
                # which an HTML parser would read as an open tag.
                child.appendChild(child.ownerDocument.createTextNode(""))
            skeletonize(child)


def resolve(doc, fragment):
    """Resolve the fragment of a stored position: `kobo.N.M` (element id) or
    `point(/a/b/…:offset)` (1-based indices over all child nodes, /1 = html)."""
    if fragment.startswith("point("):
        steps, offset = fragment[len("point("):-1].split(":")
        indices = [int(i) for i in steps.strip("/").split("/")]
        if indices[0] != 1:
            sys.exit(f"point path does not start at the html element: {fragment}")
        node = doc.documentElement
        for index in indices[1:]:
            node = node.childNodes[index - 1]
        if node.nodeType != node.TEXT_NODE or len(node.data) < int(offset):
            sys.exit(f"{fragment} does not end in a text node with {offset} characters")
        return ("text", len(node.data))
    matches = [e for e in doc.getElementsByTagName("*") if e.getAttribute("id") == fragment]
    if len(matches) != 1:
        sys.exit(f"id {fragment!r} found {len(matches)} times")
    return (matches[0].tagName, len(matches[0].toxml()))


def placeholder_page(title, index):
    return (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<html xmlns="http://www.w3.org/1999/xhtml"><head><title>'
        f"{title}</title></head><body><p>Placeholder for spine item {index}.</p></body></html>\n"
    )


def build_opf(opf_doc, name, opf_dir, kept_extra):
    package = opf_doc.documentElement
    version = package.getAttribute("version")
    uid_name = zero_digits(package.getAttribute("unique-identifier"))
    language = opf_doc.getElementsByTagNameNS(DC_NS, "language")
    language = language[0].firstChild.data.strip() if language and language[0].firstChild else "en"
    manifest = {i.getAttribute("id"): i for i in opf_doc.getElementsByTagNameNS(OPF_NS, "item")}
    spine = opf_doc.getElementsByTagNameNS(OPF_NS, "spine")[0]
    itemrefs = spine.getElementsByTagNameNS(OPF_NS, "itemref")

    def attrs(element, skip=()):
        return "".join(
            f' {k}="{zero_digits(v)}"' for k, v in element.attributes.items() if k not in skip and not k.startswith("xmlns")
        )

    items, refs, spine_hrefs = [], [], []
    for ref in itemrefs:
        item = manifest[ref.getAttribute("idref")]
        items.append(f"    <item{attrs(item)}/>")
        refs.append(f"    <itemref{attrs(ref)}/>")
        spine_hrefs.append(item.getAttribute("href"))
    for item in manifest.values():
        if item.getAttribute("href") in kept_extra and item.getAttribute("href") not in spine_hrefs:
            items.append(f"    <item{attrs(item)}/>")
    book_uuid = uuid.uuid5(uuid.NAMESPACE_URL, f"nickelinksync-fixture-book-{name}")
    modified = '    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>\n' if version.startswith("3") else ""
    opf = (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        f'<package xmlns="{OPF_NS}" version="{version}" unique-identifier="{uid_name}">\n'
        f'  <metadata xmlns:dc="{DC_NS}">\n'
        f'    <dc:identifier id="{uid_name}">urn:uuid:{book_uuid}</dc:identifier>\n'
        f"    <dc:title>Book {name.upper()}</dc:title>\n"
        f"    <dc:creator>Author {name.upper()}</dc:creator>\n"
        f"    <dc:language>{language}</dc:language>\n"
        f"{modified}"
        "  </metadata>\n"
        "  <manifest>\n" + "\n".join(items) + "\n  </manifest>\n"
        f"  <spine{attrs(spine)}>\n" + "\n".join(refs) + "\n  </spine>\n"
        "</package>\n"
    )
    return opf, spine_hrefs


def text_words(data, attributes):
    """Words of five or more letters in the text (and attribute values) of an XML file."""
    words, doc = set(), minidom.parseString(data)
    stack = [doc]
    while stack:
        node = stack.pop()
        if node.nodeType in (node.TEXT_NODE, node.CDATA_SECTION_NODE, node.COMMENT_NODE):
            # Style sheets and scripts are tooling (kepubify), not book text.
            if attributes or node.parentNode.nodeName not in ("style", "script"):
                words |= set(re.findall(r"[^\W\d_]{5,}", node.data))
        if attributes and node.nodeType == node.ELEMENT_NODE:
            for attr in node.attributes.values():
                if attr.name == "epub:type":
                    continue  # EPUB vocabulary such as "cover", not book text.
                words |= set(re.findall(r"[^\W\d_]{5,}", attr.value))
        stack.extend(node.childNodes)
    return words


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("source")
    parser.add_argument("output")
    parser.add_argument("--name", required=True, help="placeholder letter, e.g. a for Book A")
    parser.add_argument("--keep", action="append", default=[], help="chapter (path in the EPUB) to keep as a skeleton")
    parser.add_argument("--position", action="append", default=[], help="stored position to check, path#fragment")
    args = parser.parse_args()

    src = zipfile.ZipFile(args.source)
    container = minidom.parseString(src.read("META-INF/container.xml"))
    opf_path = container.getElementsByTagNameNS(CONTAINER_NS, "rootfile")[0].getAttribute("full-path")
    opf_dir = posixpath.dirname(opf_path)
    opf_doc = minidom.parseString(src.read(opf_path))
    manifest = {i.getAttribute("id"): i.getAttribute("href") for i in opf_doc.getElementsByTagNameNS(OPF_NS, "item")}
    navs = [i.getAttribute("href") for i in opf_doc.getElementsByTagNameNS(OPF_NS, "item")
            if "nav" in i.getAttribute("properties").split()]
    toc_id = opf_doc.getElementsByTagNameNS(OPF_NS, "spine")[0].getAttribute("toc")
    tocs = ([manifest[toc_id]] if toc_id else []) + navs
    opf, spine_hrefs = build_opf(opf_doc, args.name, opf_dir, set(tocs))
    in_book = lambda href: posixpath.join(opf_dir, href)  # noqa: E731

    title = f"Book {args.name.upper()}"
    files = {"META-INF/container.xml": zero_digits(src.read("META-INF/container.xml").decode("utf-8"))}
    files[zero_digits(opf_path)] = opf
    skeletons = set(args.keep) | {in_book(t) for t in tocs}
    for index, href in enumerate(spine_hrefs):
        if in_book(href) not in skeletons:
            files[zero_digits(in_book(href))] = placeholder_page(title, index)
    originals = {}
    for path in sorted(skeletons):
        if path not in src.namelist():
            sys.exit(f"{path} is not in the EPUB")
        originals[path] = src.read(path)
        doc = minidom.parseString(originals[path])
        skeletonize(doc)
        files[zero_digits(path)] = doc.toxml(encoding="utf-8").decode("utf-8") + "\n"

    for position in args.position:
        path, fragment = position.split("#", 1)
        if path not in skeletons:
            sys.exit(f"{position}: chapter is not kept with --keep")
        before = resolve(minidom.parseString(originals[path]), fragment)
        after = resolve(minidom.parseString(files[zero_digits(path)]), fragment)
        if before[0] != after[0] or (before[0] == "text" and before[1] != after[1]):
            sys.exit(f"{position}: resolves to {after} in the fixture, {before} in the source")
        print(f"{position}: resolves to the same {before[0]} node")

    # Leftovers: original text words, metadata, long numbers.
    forbidden = set()
    for path in originals:
        forbidden |= text_words(originals[path], attributes=False)
    for node in opf_doc.getElementsByTagNameNS(DC_NS, "*"):
        if node.firstChild is not None and node.localName != "language":
            forbidden |= set(re.findall(r"[^\W\d_]{5,}", node.firstChild.data))
            forbidden.add(node.firstChild.data.strip())
    found = set()
    for path, data in files.items():
        if path.endswith((".xhtml", ".html", ".ncx", ".opf", ".xml")):
            found |= text_words(data.encode("utf-8"), attributes=True) & forbidden
        found |= {s for s in forbidden if len(s) >= 8 and s in data}
        if re.search(r"[1-9]\d{9,}", data):
            found.add(f"long number in {path}")
    if found:
        sys.exit(f"Leftovers of the original book in the output, nothing written: {len(found)} items, e.g. "
                 + ", ".join(repr(w[:3] + "…") for w in sorted(found)[:5]))

    with zipfile.ZipFile(args.output, "w") as out:
        out.writestr(zipfile.ZipInfo("mimetype", ZIP_TIME), "application/epub+zip", zipfile.ZIP_STORED)
        for path in sorted(files):
            info = zipfile.ZipInfo(path, ZIP_TIME)
            info.external_attr = 0o644 << 16
            out.writestr(info, files[path], zipfile.ZIP_DEFLATED, compresslevel=9)
    print(f"{args.output}: {len(spine_hrefs)} spine items, kept {len(skeletons)} files as skeletons, no leftovers found")


if __name__ == "__main__":
    main()
