#!/usr/bin/env python3
"""Build the CJK font slices the editor and the exporters load on demand.

`XL-FEAT-140`. The bundled faces are Latin-only; Japanese, Chinese and Korean
text needs a face per region (the same code point is drawn differently in each),
and a whole CJK face is 10-25 MB. So each region's Noto face is instanced to
static Regular (400) and Bold (700), cut into **unicode-range slices** of a few
hundred characters, and written as WOFF2. A browser fetches only the slices a
sheet's text touches; the PDF/PNG exporters are handed the same files by the
host. Nothing here is compiled into the WebAssembly binary.

Reproducible and pinned:

- **Sources** are the variable Noto fonts in google/fonts at one commit, each
  checked against a SHA-256 below. A changed download fails the build.
- **Slicing** is `tools/fonts/cjk-ranges.json`, committed: Google Fonts' own
  frequency-ordered partition for each region (so a typical sentence touches a
  handful of slices, not one per character), followed by "tail" slices that
  cover every remaining character the face has — Google Fonts drops rare Han
  from its slicing, and a sheet with a rare name in it must still render.
  `--refresh-ranges` re-fetches that partition; nothing else touches the
  network.
- **Output** is byte-deterministic for a given fontTools version (pinned below):
  timestamps are not recalculated and brotli is deterministic.

WOFF2 is written with the **null transform** for `glyf`/`loca` (WOFF2 §5.1,
transform version 3). That costs a few percent of size and buys a decoder that
is brotli plus table reassembly — which is what `casual-calc-text` carries to
read these in WebAssembly — instead of the glyf reconstruction algorithm.

Usage:
    python3 tools/fonts/build-cjk-slices.py [--cache DIR] [--only jp-sans-400 ...]
    python3 tools/fonts/build-cjk-slices.py --refresh-ranges
"""

from __future__ import annotations

import argparse
import concurrent.futures as cf
import hashlib
import io
import json
import os
import re
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RANGES = ROOT / "tools" / "fonts" / "cjk-ranges.json"
OUT = ROOT / "webapp" / "fonts" / "cjk"

FONTTOOLS_VERSION = "4.60.2"
GOOGLE_FONTS_COMMIT = "47d54ca1ebb14fd864695f1d39dc734f39017c33"

# (region, style) -> (path in google/fonts, sha256 of the variable TTF)
SOURCES = {
    ("jp", "sans"): ("ofl/notosansjp/NotoSansJP[wght].ttf",
                     "c2f3b4d463500a2ddcd3849cded1fceeb9fd6d1c32e6cbecd568453ba50fc68f"),
    ("jp", "serif"): ("ofl/notoserifjp/NotoSerifJP[wght].ttf",
                      "2fd527ba12b6a44ec30d796d633360da0aeba6c5d4af1304ce12bb4dc15a7dfc"),
    ("sc", "sans"): ("ofl/notosanssc/NotoSansSC[wght].ttf",
                     "a3041811a78c361b1de50f953c805e0244951c21c5bd412f7232ef0d899af0da"),
    ("sc", "serif"): ("ofl/notoserifsc/NotoSerifSC[wght].ttf",
                      "050080d9255a86808f2945bffac582b31ef32bc36411ce29563b4961670c66f9"),
    ("tc", "sans"): ("ofl/notosanstc/NotoSansTC[wght].ttf",
                     "864727d210d54f2537bbe23b3a839436c3992af72de9322af5270897246bd44f"),
    ("tc", "serif"): ("ofl/notoseriftc/NotoSerifTC[wght].ttf",
                      "0077e18f57c6908f4a000969880940bdb0dad057c0e8d98b49dc364c3d1b09c6"),
    ("kr", "sans"): ("ofl/notosanskr/NotoSansKR[wght].ttf",
                     "194018e6b2b293a7964f037b25c0249ce1418bc9ab3c971060a03aa57861e252"),
    ("kr", "serif"): ("ofl/notoserifkr/NotoSerifKR[wght].ttf",
                      "11f8d5de6f1b79195efba3828aaa2ec95c1178f5ae976fb23c8d53250a9938f3"),
}
# The licence texts, one per family: the sans faces are Adobe's Source Han
# design (Reserved Font Name "Source" — which is why the CSS families here are
# not called that), the serif ones Google's. Identical across the four regions
# of each, which the pins assert.
NOTICES = {
    "OFL-Noto-Sans-CJK.txt": ("ofl/notosansjp/OFL.txt",
                              "1c05c68c34f9708415aada51f17e1b0092d2cea709bf4a94cd38114f9e73d7d9"),
    "OFL-Noto-Serif-CJK.txt": ("ofl/notoserifjp/OFL.txt",
                               "5e0da210fb04058a8c0087985d2d456b931c2579811a49655721d3cf0c36b6d6"),
}

# Sans in Regular and Bold; serif in Regular only. Excel's own serif CJK faces
# (MS Mincho, SimSun, Batang, PMingLiU) have no bold either, and Windows draws
# their bold by emboldening the regular outlines — which is what the canvas does
# with `font-synthesis` and what the exporters do with a stroked fill. Shipping
# a real serif bold would add about a quarter to the total for a weight Excel
# itself synthesises.
WEIGHTS = {"sans": (400, 700), "serif": (400,)}
# CSS family names. Our own, not "Noto Sans JP": a machine that has Noto
# installed must not have its copy chosen over these in a way that changes
# metrics between two viewers, and the OFL asks that modified versions not
# trade on the original's name.
FAMILY = {
    ("jp", "sans"): "OpenCalc CJK JP Sans", ("jp", "serif"): "OpenCalc CJK JP Serif",
    ("sc", "sans"): "OpenCalc CJK SC Sans", ("sc", "serif"): "OpenCalc CJK SC Serif",
    ("tc", "sans"): "OpenCalc CJK TC Sans", ("tc", "serif"): "OpenCalc CJK TC Serif",
    ("kr", "sans"): "OpenCalc CJK KR Sans", ("kr", "serif"): "OpenCalc CJK KR Serif",
}
# The Google Fonts family whose slicing each region uses (the serif faces share it).
GF_FAMILY = {"jp": "Noto Sans JP", "sc": "Noto Sans SC", "tc": "Noto Sans TC", "kr": "Noto Sans KR"}
TAIL_SLICE = 400  # characters per tail slice
# Only what horizontal text needs. The vertical (`vert`/`vrt2`) and
# proportional (`palt`) features would pull alternate glyphs into every slice;
# vertical text is XL-FEAT-141's and will add them back deliberately.
LAYOUT_FEATURES = ["ccmp", "kern", "mark", "mkmk"]


def fetch(path: str, sha: str, cache: Path) -> bytes:
    cache.mkdir(parents=True, exist_ok=True)
    local = cache / (sha + "-" + os.path.basename(path).replace("[", "_").replace("]", "_"))
    if local.exists():
        data = local.read_bytes()
    else:
        url = ("https://raw.githubusercontent.com/google/fonts/" + GOOGLE_FONTS_COMMIT + "/"
               + path.replace("[", "%5B").replace("]", "%5D"))
        print(f"fetch {url}", file=sys.stderr)
        data = urllib.request.urlopen(url).read()
    got = hashlib.sha256(data).hexdigest()
    if got != sha:
        sys.exit(f"{path}: sha256 {got} does not match the pin {sha}")
    local.write_bytes(data)
    return data


def parse_ranges(text: str) -> list[tuple[int, int]]:
    out = []
    for part in text.split(","):
        part = part.strip()
        if not part:
            continue
        a, _, b = part.partition("-")
        out.append((int(a, 16), int(b or a, 16)))
    return out


def format_ranges(cps: list[int]) -> str:
    cps = sorted(set(cps))
    parts, i = [], 0
    while i < len(cps):
        j = i
        while j + 1 < len(cps) and cps[j + 1] == cps[j] + 1:
            j += 1
        parts.append(f"{cps[i]:x}" if i == j else f"{cps[i]:x}-{cps[j]:x}")
        i = j + 1
    return ",".join(parts)


def refresh_ranges() -> None:
    ua = ("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 "
          "(KHTML, like Gecko) Chrome/130.0 Safari/537.36")
    regions = {}
    for region, fam in GF_FAMILY.items():
        url = ("https://fonts.googleapis.com/css2?family=" + fam.replace(" ", "+")
               + ":wght@400&display=swap")
        css = urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": ua})).read().decode()
        slices = []
        for rng in re.findall(r"unicode-range:\s*([^;]+);", css):
            slices.append(rng.replace("U+", "").replace(" ", "").lower())
        regions[region] = slices
    RANGES.write_text(json.dumps({
        "source": "fonts.googleapis.com/css2 unicode-range partition, Chrome user agent, wght 400",
        "regions": regions,
    }, indent=1) + "\n")
    print(f"wrote {RANGES}")


def partition(region: str, cmap_cps: set[int]) -> list[list[int]]:
    """The region's slices as lists of code points the faces actually have."""
    gf = json.loads(RANGES.read_text())["regions"][region]
    seen: set[int] = set()
    slices: list[list[int]] = []
    for text in gf:
        cps = []
        for a, b in parse_ranges(text):
            for cp in range(a, b + 1):
                if cp in cmap_cps and cp not in seen:
                    cps.append(cp)
                    seen.add(cp)
        if cps:
            slices.append(sorted(cps))
    rest = sorted(cmap_cps - seen)
    for i in range(0, len(rest), TAIL_SLICE):
        slices.append(rest[i:i + TAIL_SLICE])
    return slices


def instance(var_bytes: bytes, weight: int) -> bytes:
    from fontTools.ttLib import TTFont
    from fontTools.varLib import instancer
    font = TTFont(io.BytesIO(var_bytes))
    font.recalcTimestamp = False
    static = instancer.instantiateVariableFont(font, {"wght": weight}, static=True)
    buf = io.BytesIO()
    static.save(buf)
    return buf.getvalue()


def cut(args: tuple[bytes, list[int], str]) -> tuple[str, int]:
    static_bytes, cps, dest = args
    from fontTools import subset
    from fontTools.ttLib import TTFont
    from fontTools.ttLib.woff2 import WOFF2FlavorData
    font = TTFont(io.BytesIO(static_bytes))
    font.recalcTimestamp = False
    opts = subset.Options()
    opts.layout_features = LAYOUT_FEATURES
    opts.hinting = False
    opts.notdef_outline = True
    opts.name_IDs = [0, 1, 2, 3, 4, 5, 6, 13, 14]
    opts.name_languages = [0x409]
    opts.drop_tables += ["vhea", "vmtx", "VORG", "BASE", "gasp", "DSIG", "STAT"]
    sub = subset.Subsetter(opts)
    sub.populate(unicodes=cps)
    sub.subset(font)
    font.flavor = "woff2"
    font.flavorData = WOFF2FlavorData(transformedTables=())
    buf = io.BytesIO()
    font.save(buf)
    data = buf.getvalue()
    Path(dest).parent.mkdir(parents=True, exist_ok=True)
    Path(dest).write_bytes(data)
    return dest, len(data)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--cache", default=str(ROOT / "target" / "font-sources"))
    ap.add_argument("--only", nargs="*", help="variant keys such as jp-sans-400")
    ap.add_argument("--refresh-ranges", action="store_true")
    ap.add_argument("--ranges-only", action="store_true",
                    help="rewrite the {region}.ranges files from manifest.json")
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    a = ap.parse_args()
    if a.refresh_ranges:
        refresh_ranges()
        return
    if a.ranges_only:
        write_ranges(json.loads((OUT / "manifest.json").read_text()))
        write_notices(Path(a.cache))
        return
    import fontTools
    if fontTools.version != FONTTOOLS_VERSION:
        sys.exit(f"fontTools {fontTools.version}; the slices are pinned to {FONTTOOLS_VERSION} "
                 f"(pip install fonttools[woff]=={FONTTOOLS_VERSION})")
    from fontTools.ttLib import TTFont

    cache = Path(a.cache)
    OUT.mkdir(parents=True, exist_ok=True)
    write_notices(cache)

    manifest_path = OUT / "manifest.json"
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() and a.only else {}
    manifest.setdefault("regions", {})
    manifest.setdefault("variants", {})

    by_region: dict[str, list[tuple[str, bytes]]] = {}
    for (region, style), (path, sha) in SOURCES.items():
        by_region.setdefault(region, []).append((style, fetch(path, sha, cache)))

    # Partitions first (cheap), then every static instance in parallel —
    # instancing is single-threaded and is most of the build's time.
    parts: dict[str, list[list[int]]] = {}
    for region, faces in by_region.items():
        cps: set[int] = set()
        for _, data in faces:
            cps |= set(TTFont(io.BytesIO(data), lazy=True).getBestCmap().keys())
        parts[region] = partition(region, cps)
        manifest["regions"][region] = [format_ranges(s) for s in parts[region]]

    todo = []
    for region, faces in by_region.items():
        for style, data in faces:
            for weight in WEIGHTS[style]:
                key = f"{region}-{style}-{weight}"
                if not a.only or key in a.only:
                    todo.append((key, region, style, weight, data))
    statics: dict[str, bytes] = {}
    with cf.ProcessPoolExecutor(min(4, a.jobs)) as ex:
        futures = {ex.submit(instance, data, weight): key for key, _, _, weight, data in todo}
        for fut in cf.as_completed(futures):
            statics[futures[fut]] = fut.result()
            print(f"{futures[fut]}: instanced", file=sys.stderr)

    for key, region, style, weight, _ in todo:
        slices = parts[region]
        vdir = OUT / key
        for old in vdir.glob("*.woff2") if vdir.exists() else []:
            old.unlink()
        jobs = [(statics[key], s, str(vdir / f"{i:03}.woff2")) for i, s in enumerate(slices)]
        sizes = {}
        with cf.ProcessPoolExecutor(a.jobs) as ex:
            for dest, n in ex.map(cut, jobs, chunksize=4):
                sizes[Path(dest).name] = n
        files = []
        for i in range(len(slices)):
            blob = (vdir / f"{i:03}.woff2").read_bytes()
            files.append(hashlib.sha256(blob).hexdigest()[:16])
        manifest["variants"][key] = {
            "family": FAMILY[(region, style)],
            "region": region,
            "style": style,
            "weight": weight,
            "bytes": sum(sizes.values()),
            "sha256_16": files,
        }
        print(f"{key}: {len(slices)} slices, {sum(sizes.values())} bytes", file=sys.stderr)

    manifest["format"] = 1
    manifest["path"] = "{variant}/{index:03}.woff2"
    manifest["source"] = {
        "google_fonts_commit": GOOGLE_FONTS_COMMIT,
        "fonttools": FONTTOOLS_VERSION,
        "pins": {f"{r}-{s}": sha for (r, s), (_, sha) in SOURCES.items()},
    }
    manifest_path.write_text(json.dumps(manifest, sort_keys=True, separators=(",", ":")) + "\n")
    print(f"wrote {manifest_path}", file=sys.stderr)
    write_ranges(manifest)


def write_notices(cache: Path) -> None:
    """The OFL texts the slices are distributed under, beside them."""
    for name, (path, sha) in NOTICES.items():
        (OUT / name).write_bytes(fetch(path, sha, cache))
    stale = OUT / "OFL.txt"
    if stale.exists():
        stale.unlink()


def write_ranges(manifest: dict) -> None:
    """One `{region}.ranges` per region: slice N's code points on line N.

    The same data as the manifest's `regions`, in a form a host with no JSON
    parser (the Rust exporters, `casual_calc_text::cjk::SliceDirectory`) reads
    with a line split.
    """
    for region, slices in sorted(manifest["regions"].items()):
        (OUT / f"{region}.ranges").write_text("\n".join(slices) + "\n")


if __name__ == "__main__":
    main()
