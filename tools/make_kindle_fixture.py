#!/usr/bin/env python3
"""Turn copied Send-to-Kindle books into anonymized Kindle fixtures.

Reads a copy of `documents/Downloads/Items01/` from a Kindle (never the
device itself) and, for each book given with `--book`, writes

    OUTPUT/book-<x>_<id>.kfx
    OUTPUT/book-<x>_<id>.sdr/book-<x>_<id><device hash>.yjf
    OUTPUT/book-<x>_<id>.sdr/book-<x>_<id><device hash>.yjr

with the same layout as on the Kindle. Only the KFX and the two KRDS files
(`.yjf`: reading position, `.yjr`: highlights and settings) are kept; the
`.mf` and `AssetDownloadMetadata.meta` files hold signed download URLs and
are never read.

Anonymized, always to a value of the same length, so every Ion and KRDS
length field stays valid:

| Value | In the fixture |
|---|---|
| `content_id` / `ASIN` (32 hex digits, also in the file names) | `000…0001`, `000…0002`, … |
| `asset_id` (`CR!…`) | `CR!000…0001`, … |
| `book_id` | `0…01`, … |
| `issue_date` | `2026-01-01` |
| source OPF hash in `misc/resource_…_filename_opf` | zeros |
| device hash in the `.yjf`/`.yjr` file names | zeros |
| timestamps in the KRDS files (milliseconds) | `2026-01-01T12:00:00Z`, then one second apart, in their original order |

The KRDS files are parsed completely: any key or string value not known to
be harmless stops the script, so new kinds of data are never copied
silently. The KFX is parsed after the rewrite and compared with the
original, and the output is searched for every replaced value.

    tools/make_kindle_fixture.py ITEMS01_COPY tests/fixtures/kindle --book p --book q --book r
"""

import argparse
import datetime
import re
import struct
import sys
from pathlib import Path

KRDS_MAGIC = b"\x00\x00\x00\x00\x00\x1a\xb1\x26"
BASE_TIME_MS = int(datetime.datetime(2026, 1, 1, 12, tzinfo=datetime.timezone.utc).timestamp() * 1000)
# Milliseconds between 2020 and 2033: a timestamp, not a counter.
TIME_RANGE = (1_577_836_800_000, 2_000_000_000_000)

# KRDS object names and string values seen in the `.yjf`/`.yjr` of a
# Send-to-Kindle book on firmware 5.19.2. Anything else stops the script.
KNOWN_NAMES = {
    "timer.model", "timer.average.calculator", "fpr", "book.info.store",
    "page.history.store", "page.history.record", "whisperstore.migration.status",
    "lpr", "font.prefs", "sync_lpr", "next.in.series.info.data",
    "annotation.cache.object", "saved.avl.interval.tree",
    "annotation.personal.highlight", "ReaderMetrics",
}
KNOWN_STRINGS = {"", "INVALID", "_INVALID_,und:bookerly", "true", "0\ufffc0", "booklaunchedbefore"}
POSITION = re.compile(r"^[A-Za-z0-9+/]{11,16}={0,2}:\d+$")


# --- KRDS (`.yjf`, `.yjr`) -------------------------------------------------

def krds_walk(data):
    """Yield (kind, value, offset of the value) for every token of a KRDS file.

    Format: magic, then typed values: 0x00 bool (1 byte), 0x01 int (4, BE),
    0x02 long (8, BE), 0x03 string (flag byte; 0 = present, then u16 BE
    length and UTF-8), 0x04 double (8), 0x07 byte (1), 0xFE object start
    with a name in string form, 0xFF object end.
    """
    if not data.startswith(KRDS_MAGIC):
        raise SystemExit("not a KRDS file")
    i = len(KRDS_MAGIC)
    while i < len(data):
        t = data[i]
        i += 1
        if t == 0x00 or t == 0x07:
            yield "byte", data[i], i
            i += 1
        elif t == 0x01:
            yield "int", struct.unpack_from(">i", data, i)[0], i
            i += 4
        elif t == 0x02:
            yield "long", struct.unpack_from(">q", data, i)[0], i
            i += 8
        elif t == 0x04:
            yield "double", struct.unpack_from(">d", data, i)[0], i
            i += 8
        elif t in (0x03, 0xFE):
            flag = data[i]
            i += 1
            if flag == 1:
                yield ("string" if t == 0x03 else "object"), None, i
                continue
            n = struct.unpack_from(">H", data, i)[0]
            i += 2
            yield ("string" if t == 0x03 else "object"), data[i:i + n].decode("utf-8"), i
            i += n
        elif t == 0xFF:
            yield "end", None, i
        else:
            raise SystemExit(f"unknown KRDS type 0x{t:02x} at {i - 1}")


def krds_timestamps(data):
    return [(off, v) for kind, v, off in krds_walk(data)
            if kind == "long" and TIME_RANGE[0] <= v < TIME_RANGE[1]]


def check_krds(name, data):
    for kind, v, _ in krds_walk(data):
        if kind == "object" and v not in KNOWN_NAMES:
            raise SystemExit(f"{name}: unknown KRDS object {v!r}")
        if kind == "string" and v is not None and v not in KNOWN_STRINGS and not POSITION.match(v):
            raise SystemExit(f"{name}: unknown KRDS string {v!r}")


# --- Ion and the KFX container ----------------------------------------------

def _varuint(b, i):
    v = 0
    while True:
        c = b[i]
        i += 1
        v = (v << 7) | (c & 0x7F)
        if c & 0x80:
            return v, i


def ion_value(b, i):
    """Read one Ion binary value: returns (value, next offset).

    Structs become lists of (field id, value) pairs, symbols ("$", id),
    annotations ("::", ids, value); enough to compare two files.
    """
    td = b[i]
    i += 1
    t, ln = td >> 4, td & 0x0F
    if ln == 15:
        return None, i
    if t == 1:
        return bool(ln), i
    if ln == 14 or (t == 0xD and ln == 1):
        ln, i = _varuint(b, i)
    end = i + ln
    raw = bytes(b[i:end])
    if t in (2, 3):
        v = int.from_bytes(raw, "big")
        return (-v if t == 3 else v), end
    if t == 8:
        return raw.decode("utf-8"), end
    if t == 7:
        return ("$", int.from_bytes(raw, "big")), end
    if t in (0xB, 0xC):
        out = []
        while i < end:
            v, i = ion_value(b, i)
            out.append(v)
        return out, end
    if t == 0xD:
        out = []
        while i < end:
            k, i = _varuint(b, i)
            v, i = ion_value(b, i)
            out.append((k, v))
        return out, end
    if t == 0xE:
        al, j = _varuint(b, i)
        aend = j + al
        anns = []
        while j < aend:
            a, j = _varuint(b, j)
            anns.append(a)
        v, _ = ion_value(b, aend)
        return ("::", anns, v), end
    return (t, raw), end


def ion_stream(b):
    i, out = 0, []
    while i < len(b):
        if b[i:i + 4] == b"\xe0\x01\x00\xea":
            i += 4
            continue
        v, i = ion_value(b, i)
        out.append(v)
    return out


def kfx_entities(data):
    """Return the container info and all entities of a KFX container."""
    if data[:4] != b"CONT":
        raise SystemExit("not a KFX container (DRM?)")
    _, hlen, cio, cil = struct.unpack_from("<HIII", data, 4)
    info = dict(ion_stream(data[cio:cio + cil])[0])
    ito, itl = info[413], info[414]
    ents = []
    for off in range(ito, ito + itl, 24):
        eid, etype, eoff, elen = struct.unpack_from("<IIQQ", data, off)
        raw = data[hlen + eoff:hlen + eoff + elen]
        if raw[:4] != b"ENTY":
            raise SystemExit("broken entity")
        ehl = struct.unpack_from("<I", raw, 6)[0]
        payload = raw[ehl:]
        ents.append((eid, etype, payload if etype == 417 else ion_stream(payload)))
    return info, ents


def kfx_metadata(ents):
    meta = {}
    for _, etype, val in ents:
        if etype == 490:
            for cat in dict(val[0])[491]:
                for kv in dict(cat)[258]:
                    kv = dict(kv)
                    meta[kv[492]] = kv[307]
    return meta


# --- main -----------------------------------------------------------------

def same_length(old, new):
    if len(old) != len(new):
        raise SystemExit(f"replacement for {old!r} has the wrong length")
    return old.encode(), new.encode()


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("source", type=Path, help="copy of documents/Downloads/Items01")
    ap.add_argument("output", type=Path)
    ap.add_argument("--book", action="append", required=True,
                    help="letter of a fixture book (book-<letter>), repeatable")
    args = ap.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)

    for n, letter in enumerate(args.book, start=1):
        stem = f"book-{letter}"
        kfx = list(args.source.glob(f"{stem}_*.kfx"))
        if len(kfx) != 1:
            raise SystemExit(f"{stem}: expected one KFX, found {len(kfx)}")
        kfx = kfx[0]
        data = kfx.read_bytes()
        info, ents = kfx_entities(data)
        meta = kfx_metadata(ents)
        if meta.get("cde_content_type") != "PDOC" or meta.get("title") != stem:
            raise SystemExit(f"{stem}: not a Send-to-Kindle copy of the fixture book")

        cid = meta["content_id"]
        new_cid = f"{n:032d}"
        asset = meta["asset_id"]
        repl = {
            cid: new_cid,
            asset: "CR!" + f"{n:0{len(asset) - 3}d}",
            meta["book_id"]: f"{n:0{len(meta['book_id'])}d}",
            meta["issue_date"]: "2026-01-01",
        }
        for _, etype, val in ents:
            if etype == 597:
                for m in re.finditer(r"resource_\d+_\d+_([0-9a-f]{16})_", str(val)):
                    repl[m.group(1)] = "0" * 16
        if info.get(409) != asset:
            raise SystemExit(f"{stem}: container asset id differs from the metadata")

        out = data
        for old, new in repl.items():
            o, w = same_length(old, new)
            out = out.replace(o, w)

        # The rewrite must leave everything but the replaced strings as is.
        def scrub(v):
            if isinstance(v, str):
                for old, new in repl.items():
                    v = v.replace(old, new)
                return v
            if isinstance(v, list):
                return [scrub(x) for x in v]
            if isinstance(v, tuple):
                return tuple(scrub(x) for x in v)
            if isinstance(v, dict):
                return {k: scrub(x) for k, x in v.items()}
            return v
        if kfx_entities(out) != scrub((info, ents)):
            raise SystemExit(f"{stem}: KFX changed beyond the replaced values")

        name = f"{stem}_{new_cid}"
        (args.output / f"{name}.kfx").write_bytes(out)
        sdr_out = args.output / f"{name}.sdr"
        sdr_out.mkdir(exist_ok=True)

        sdr = kfx.with_suffix(".sdr")
        krds = {}
        for ext in (".yjf", ".yjr"):
            found = [p for p in sdr.glob(f"{kfx.stem}*{ext}")]
            if len(found) != 1:
                raise SystemExit(f"{stem}: expected one {ext}, found {len(found)}")
            krds[ext] = bytearray(found[0].read_bytes())
            check_krds(found[0].name, krds[ext])
        stamps = sorted({v for b in krds.values() for _, v in krds_timestamps(b)})
        new_time = {v: BASE_TIME_MS + 1000 * k for k, v in enumerate(stamps)}
        for ext, b in krds.items():
            for off, v in krds_timestamps(b):
                struct.pack_into(">q", b, off, new_time[v])
            (sdr_out / f"{name}{'0' * 32}{ext}").write_bytes(bytes(b))

        written = [p.read_bytes() for p in [args.output / f"{name}.kfx", *sdr_out.iterdir()]]
        for old in repl:
            if any(old.encode() in w for w in written):
                raise SystemExit(f"{stem}: {old!r} left in the output")
        for v in stamps:
            if any(struct.pack(">q", v) in w for w in written):
                raise SystemExit(f"{stem}: original timestamp left in the output")
        print(f"{stem}: {name}.kfx, {len(stamps)} timestamps, "
              f"{len(repl)} values replaced", file=sys.stderr)


if __name__ == "__main__":
    main()
