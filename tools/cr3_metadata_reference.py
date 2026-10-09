"""Ground-truth table for the CR3 metadata parity test (crates/raw/tests/cr3_metadata_parity.rs).

Parses ExifTool `-a -U -G1 -H -v3` text dumps of the gitignored CC0 CR3 corpus (run once,
outside the repository; the test never invokes ExifTool) and regenerates the committed
`Entry` table between its BEGIN/END GENERATED TABLE markers. With `--matrix lightcraft.jsonl`
it also prints the per-body parity matrix for `docs/cr3-metadata-parity.md`, comparing the
ground truth against LightCraft's own extraction dump (`cargo test -p lightcraft-raw
--test cr3_metadata_parity dump -- --ignored --nocapture`).

Usage:
    python tools/cr3_metadata_reference.py <exiftool-dump-dir> [--update test-file.rs]
    python tools/cr3_metadata_reference.py <exiftool-dump-dir> --matrix lightcraft-dump.jsonl

ExifTool's verbose dump prints each ColorData word field with its hex word offset
(`- Tag 0x0047` = word 71), which is how the per-version offsets in the generated table
were cross-checked against the decoder's (cr3.rs `camera_levels`).

Ground truth mapping (ExifTool tag -> LightCraft field):
    WB_RGGBLevelsAsShot   -> wb_multipliers, normalized to green = 1 ([R, B])
    PerChannelBlackLevel  -> BlackLevel (2x2 grid, re-ordered per CFA/active-area phase)
    SpecularWhiteLevel    -> white (single scalar; NormalWhiteLevel is NOT used)
    SensorWidth/Height    -> full sensor dimensions (CMP1 track dims)
    SensorInfo borders    -> SensorInfo-based geometry fallback (IAD1 preferred by the decoder)
    ExifImageWidth/Height -> developed (recommended-crop) size, the aspect information
    LensModel (0xa434)    -> metadata.lens_model (CMT3 0x0095 agrees on every corpus file)
    Model / CanonModelID  -> body identity
"""
import argparse
import glob
import json
import os
import re
import struct
import sys

# Representative selection: one base file per Model string, plus the variants that exercise
# distinct metadata paths (crop mode, dual pixel, aspect ratio, the unparsed ColorData version).
EXTRA_PATTERNS = [
    "cr3-canon-eos-r5m2-aps-c-raw",             # APS-C crop mode
    "cr3-eos-r5-canon-eos-r5-raw-iso-100-crop-nodual",  # R5 crop mode
    "cr3-eos-r5-canon-eos-r5-raw-iso-100-nocrop-dual",  # R5 dual pixel
    "cr3-eos-r-canon-eos-r-raw-iso-100-crop-nodual",    # EOS R crop mode
    "cr3-eos-r-canon-eos-r-raw-iso-100-dual-pixel-on",  # EOS R dual pixel
    "cr3-eos-r8-raw-full-frame-dual-pixel",     # R8 dual pixel
    "cr3-eos-r6-canon-eos-r6-raw-iso-100-crop-nodual",  # R6 crop mode
    "cr3-eos-r6-mark-ii-canon-r6m2-img-0349-raw-dual-crop",  # R6m2 dual + crop
    "cr3-eos-rp-canon-eos-rp-raw-iso-100-crop-nodual",  # RP crop mode
    "cr3-eos-250d-canon250d-craw-3-2",          # aspect: full 3:2
    "cr3-eos-250d-canon250d-raw-19-9",          # aspect: cinema
    "cr3-eos-250d-canon250d-raw-2-3",           # aspect: portrait
    "cr3-eos-m6-mark-ii-img-raw-3-2",           # second RAW-compression body sample
    "cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-raw-3-2",
]

INDEXED = re.compile(r"^\s*\|[\s|]*(\d+)\)\s+([A-Za-z][A-Za-z0-9 ]*?)\s*=\s*(.*)$")
BINARY = re.compile(r"^\s*\|[\s|]*([A-Za-z][A-Za-z0-9_]*)\s*=\s*(.*)$")
TAG = re.compile(r"^\s*\|[\s|]*-\s+Tag\s+0x([0-9a-f]+)")


def parse_dump(path):
    """Extract the ground-truth fields from one -v3 dump."""
    with open(path, encoding="utf-8", errors="replace") as f:
        lines = f.readlines()
    out = {}
    for i, line in enumerate(lines):
        m = INDEXED.match(line)
        if m:
            name, value = m.group(2), m.group(3).strip()
        else:
            m = BINARY.match(line)
            if not m:
                continue
            name, value = m.group(1), m.group(2).strip()
        nxt = TAG.match(lines[i + 1]) if i + 1 < len(lines) else None
        tag = int(nxt.group(1), 16) if nxt else None
        if name == "LensModel":
            key = "lens_exif" if tag == 0xA434 else "lens_maker" if tag == 0x0095 else None
            if key and key not in out:
                out[key] = value
        elif name in ("Model", "ExifImageWidth", "ExifImageHeight", "CanonModelID",
                      "ColorDataVersion", "WB_RGGBLevelsAsShot", "PerChannelBlackLevel",
                      "SpecularWhiteLevel", "SensorWidth", "SensorHeight", "SensorLeftBorder",
                      "SensorTopBorder", "SensorRightBorder", "SensorBottomBorder",
                      "AspectRatio", "CroppedImageWidth", "CroppedImageHeight"):
            out.setdefault(name, value)
    return out


def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


def multipliers(wb):
    """ExifTool's four RGGB levels -> LightCraft's green-normalized [R, B] multipliers."""
    r, g1, g2, b = (f32(float(v)) for v in wb.split())
    green = f32(f32(g1 + g2) * f32(0.5))
    return [f32(r / green), f32(b / green)]


def pick(dumps):
    """One base file per Model (plain non-CRAW preferred) plus the EXTRA_PATTERNS variants."""
    bases = {}
    for stem, d in dumps.items():
        stripped = stem.replace("nocrop", "").replace("nodual", "")
        plain = not any(s in stripped for s in ("crop", "dual", "aps-c"))
        rank = (not plain, "craw" in stem, stem)
        if d["Model"] not in bases or rank < bases[d["Model"]][0]:
            bases[d["Model"]] = (rank, stem)
    chosen = {stem for _, stem in bases.values()}
    for pattern in EXTRA_PATTERNS:
        hits = [s for s in dumps if s.startswith(pattern)]
        if not hits:
            sys.exit(f"pattern not found in dumps: {pattern}")
        chosen.add(hits[0])
    return sorted(chosen)


def rust_str(s):
    return '"%s"' % s.replace("\\", "\\\\").replace('"', '\\"')


def rust_table(entries):
    out = ["const ENTRIES: &[Entry] = &["]
    for e in entries:
        wb = "Some([%r, %r])" % (e["wb"][0], e["wb"][1]) if e["wb"] else "None"
        black = "Some([%s])" % ", ".join("%.1f" % v for v in e["black"]) if e["black"] else "None"
        white = "Some(%.1f)" % e["white"] if e["white"] else "None"
        out.append(
            "    Entry { file: %s, model: %s, color_data: %d, dpr: %s, wb: %s, black: %s, white: %s, sensor: (%d, %d), developed: (%d, %d), aspect: (%d, %d), lens: %s, model_id: %d },"
            % (rust_str(e["file"]), rust_str(e["model"]), e["cd"], "true" if e["dpr"] else "false", wb, black, white,
               e["sw"], e["sh"], e["dw"], e["dh"], e["aspect"][0], e["aspect"][1], rust_str(e["lens"]), e["mid"]))
    out.append("];")
    return "\n".join(out)


def matrix(entries, lightcraft):
    """Markdown rows: ground truth vs LightCraft's own extraction dump, one per entry.

    The dump is strict JSON (the Rust `dump_lightcraft_fields` test); decode failures carry
    `"error"` and null levels. "known" marks a deviation the committed table documents as a gap
    (KNOWN_DECODE_GAPS / KNOWN_DEVELOPED_GAPS in the test) rather than an unnoticed mismatch.
    """
    def close(pairs):
        return all(abs(a - b) <= abs(b) * 1e-3 for a, b in pairs)

    known_dev = {"cr3-canon-eos-r5m2-aps-c-raw.cr3"}     # keep in sync with KNOWN_DEVELOPED_GAPS
    known_decode = {"cr3-canon-powershot-v1-img-1225.cr3"}  # keep in sync with KNOWN_DECODE_GAPS
    rows = ["| Body | File (variant) | ColorData | Decode | As-shot WB (gt / lc) | Black 4ch (gt / lc) | White (gt / lc) | Developed (gt / lc) | Lens |",
            "|---|---|---|---|---|---|---|---|---|"]
    for e in entries:
        lc = lightcraft.get(e["file"])
        if not lc:
            rows.append("| %s | `%s` | %d | ? no dump | | | | | |" % (e["model"], e["file"], e["cd"]))
            continue
        err = lc.get("error")
        if err:
            dec = "FAIL: %s%s" % (err.rsplit(": ", 1)[-1], " (known gap)" if e["file"] in known_decode else " UNEXPECTED")
        else:
            dec = "ok"
        if e["wb"]:
            wb = "R %.4f B %.4f" % tuple(e["wb"])
            lc_wb = "R %.4f B %.4f" % (lc["wb"][0], lc["wb"][2]) if lc.get("wb") else "absent"
            ok_wb = "match" if lc.get("wb") and close(list(zip(e["wb"], [lc["wb"][0], lc["wb"][2]]))) else "MISMATCH"
        else:
            wb = lc_wb = "absent (ColorData v%d unparsed)" % e["cd"]
            ok_wb = "documented gap" if not lc.get("wb") else "MISMATCH (unexpected WB)"
        if e["black"] and e["white"]:
            bl = "/".join("%g" % v for v in e["black"])
            if lc.get("black"):
                lc_bl = "/".join("%g" % v for v in sorted(lc["black"]))
                ok_bl = "match" if sorted(lc["black"]) == sorted(e["black"]) else "MISMATCH"
            else:
                lc_bl, ok_bl = "n/a (no decode)", "n/a"
            wh, lc_wh = "%g" % e["white"], ("%g" % lc["white"] if lc.get("white") else "n/a (no decode)")
            ok_wh = "match" if lc.get("white") == e["white"] else "MISMATCH" if lc.get("white") else "n/a"
        else:
            bl = lc_bl = "sample-derived"
            ok_bl = "n/a"
            wh = lc_wh = "sample-derived"
            ok_wh = "n/a"
        dev = "%dx%d" % (e["dw"], e["dh"])
        lc_dev = "%dx%d" % tuple(lc["developed"])
        if tuple(lc["developed"]) == (e["dw"], e["dh"]):
            ok_dev = "match"
        else:
            ok_dev = "known gap" if e["file"] in known_dev else "MISMATCH"
        lens = "match" if lc["lens"] == e["lens"] else "MISMATCH (%s)" % lc["lens"]
        rows.append("| %s | `%s`%s | %d | %s | %s / %s (%s) | %s / %s (%s) | %s / %s (%s) | %s / %s (%s) | %s |"
                    % (e["model"], e["file"], " DPR" if e["dpr"] else "", e["cd"], dec,
                       wb, lc_wb, ok_wb, bl, lc_bl, ok_bl, wh, lc_wh, ok_wh, dev, lc_dev, ok_dev, lens))
    return "\n".join(rows)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dump_dir")
    ap.add_argument("--update", metavar="TEST_RS")
    ap.add_argument("--matrix", metavar="LIGHTCRAFT_JSONL")
    args = ap.parse_args()

    dumps = {}
    for path in sorted(glob.glob(os.path.join(args.dump_dir, "*.txt"))):
        stem = os.path.splitext(os.path.basename(path))[0]
        d = parse_dump(path)
        d["Model"] = d.get("Model", "?")
        dumps[stem] = d
    if not dumps:
        sys.exit(f"no dumps under {args.dump_dir}")

    entries = []
    for stem in pick(dumps):
        d = dumps[stem]
        cd = int(d.get("ColorDataVersion", 0))
        wb = multipliers(d["WB_RGGBLevelsAsShot"]) if "WB_RGGBLevelsAsShot" in d else None
        # The EOS R6 Mark III (ColorDataVersion 66) is outside the decoder's parsed versions:
        # the ground truth exists but LightCraft must report no as-shot WB until v66 lands.
        known = cd in (16, 17, 18, 19, 32, 33, 34, 48, 64, 65)
        entries.append({
            "file": stem + ".cr3", "model": d["Model"], "cd": cd,
            "dpr": "dual" in stem and "nodual" not in stem,
            "wb": wb if known else None,
            "black": [float(v) for v in d["PerChannelBlackLevel"].split()] if known else None,
            "white": float(d["SpecularWhiteLevel"]) if known else None,
            "sw": int(d["SensorWidth"]), "sh": int(d["SensorHeight"]),
            "dw": int(d["ExifImageWidth"]), "dh": int(d["ExifImageHeight"]),
            # AspectInfo (CMT3): the camera-applied aspect crop. Absent/zero AspectRatio means
            # the full ExifImageWidth x Height crop; an aspect mode shrinks it.
            "aspect": (int(d.get("CroppedImageWidth", 0)), int(d.get("CroppedImageHeight", 0)))
            if int(d.get("AspectRatio", 0) or 0) else (0, 0),
            "lens": (d.get("lens_exif") or "").strip() or (d.get("lens_maker") or "").strip(),
            "lens_exif": (d.get("lens_exif") or "").strip(),
            "lens_maker": (d.get("lens_maker") or "").strip(),
            "mid": int(d.get("CanonModelID", 0)),
        })

    if args.update:
        with open(args.update, encoding="utf-8") as f:
            src = f.read()
        begin = src.index("// BEGIN GENERATED TABLE")
        head = src[:src.index("const ENTRIES", begin)]
        tail = src[src.index("// END GENERATED TABLE"):]
        # Render the table *before* opening the file for writing: a generation error must not
        # truncate the test.
        rendered = head + rust_table(entries) + "\n" + tail
        with open(args.update, "w", encoding="utf-8", newline="\n") as f:
            f.write(rendered)
        print(f"wrote {len(entries)} entries into {args.update}", file=sys.stderr)
    elif args.matrix:
        lightcraft = {}
        with open(args.matrix, encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if line.startswith("{"):
                    row = json.loads(line)
                    lightcraft[row["file"]] = row
        print(matrix(entries, lightcraft))
    else:
        print(rust_table(entries))
    both = [e for e in entries if e["lens_exif"] and e["lens_maker"] and e["lens_exif"] != e["lens_maker"]]
    if both:
        print(f"note: ExifIFD/maker LensModel differ for {[e['file'] for e in both]}", file=sys.stderr)


if __name__ == "__main__":
    main()
