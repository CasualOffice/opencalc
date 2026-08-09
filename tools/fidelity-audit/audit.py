#!/usr/bin/env python3
"""Measure SpreadsheetML fidelity: every element and every attribute the schema
declares, against what the importer reads and the exporter writes.

The point of this tool is that its "what should exist" side comes from the
vendored ECMA-376 schema rather than from anybody's recollection. A construct
nobody remembered still shows up, which is how tables, the 1904 epoch and the
run of "cosmetic" P2 items were missed by eye.

Coverage is reported per *element in context*: the importer reads attributes
inside the match arm for a given element, so reading `ref` on `<mergeCell>` does
not count as handling `ref` on `<hyperlink>`. A flat name set would score those
as covered and quietly hide the gap this tool exists to find.
"""
import json, pathlib, re, sys, xml.etree.ElementTree as ET
from collections import defaultdict

XS = "{http://www.w3.org/2001/XMLSchema}"
ROOT = pathlib.Path(__file__).resolve().parents[2]

# The parts we claim to model semantically. Drawings, pivots and VBA are
# preserve-only by design and are audited separately.
ROOTS = {
    "worksheet": "CT_Worksheet",
    "workbook": "CT_Workbook",
    "styleSheet": "CT_Stylesheet",
    "sst": "CT_Sst",
    "table": "CT_Table",
    "comments": "CT_Comments",
}


def load_schema(path):
    root = ET.parse(path).getroot()
    ctypes, groups, agroups, top = {}, {}, {}, {}
    for ct in root.findall(f"{XS}complexType"):
        if ct.get("name"):
            ctypes[ct.get("name")] = ct
    for g in root.findall(f"{XS}group"):
        if g.get("name"):
            groups[g.get("name")] = g
    for g in root.findall(f"{XS}attributeGroup"):
        if g.get("name"):
            agroups[g.get("name")] = g
    for e in root.findall(f"{XS}element"):
        if e.get("name"):
            top.setdefault(e.get("name"), e.get("type"))
    return ctypes, groups, agroups, top


def children(node, ctypes, groups, top, seen=frozenset()):
    out = []
    for ch in node:
        tag = ch.tag.replace(XS, "")
        if tag == "element":
            ref = ch.get("ref")
            name = ch.get("name") or (ref.split(":")[-1] if ref else None)
            typ = ch.get("type") or (top.get(name) if ref else None)
            if name:
                out.append((name, typ))
        elif tag in ("sequence", "choice", "all", "complexContent", "simpleContent"):
            out += children(ch, ctypes, groups, top, seen)
        elif tag == "group":
            ref = (ch.get("ref") or "").split(":")[-1]
            if ref in groups and ref not in seen:
                out += children(groups[ref], ctypes, groups, top, seen | {ref})
        elif tag == "extension":
            base = (ch.get("base") or "").split(":")[-1]
            if base in ctypes:
                out += children(ctypes[base], ctypes, groups, top, seen)
            out += children(ch, ctypes, groups, top, seen)
    return out


def attributes(node, ctypes, agroups, seen=frozenset()):
    out = []
    for ch in node:
        tag = ch.tag.replace(XS, "")
        if tag == "attribute":
            n = ch.get("name") or (ch.get("ref") or "").split(":")[-1]
            if n:
                out.append(n)
        elif tag == "attributeGroup":
            ref = (ch.get("ref") or "").split(":")[-1]
            if ref in agroups and ref not in seen:
                out += attributes(agroups[ref], ctypes, agroups, seen | {ref})
        elif tag in ("complexContent", "simpleContent"):
            out += attributes(ch, ctypes, agroups, seen)
        elif tag == "extension":
            base = (ch.get("base") or "").split(":")[-1]
            if base in ctypes:
                out += attributes(ctypes[base], ctypes, agroups, seen)
            out += attributes(ch, ctypes, agroups, seen)
    return out


def walk(schema_path):
    """{part: {element: [attribute, ...]}} for everything reachable from a root."""
    ctypes, groups, agroups, top = load_schema(schema_path)
    parts = {}
    for part, ctname in ROOTS.items():
        found, stack, seen = {}, [(part, ctname)], set()
        while stack:
            name, typ = stack.pop()
            if not typ or typ not in ctypes:
                found.setdefault(name, [])
                continue
            ct = ctypes[typ]
            found.setdefault(name, sorted(set(attributes(ct, ctypes, agroups))))
            if typ in seen:
                continue
            seen.add(typ)
            stack += children(ct, ctypes, groups, top)
        parts[part] = found
    return parts


ATTR_READ = re.compile(r'(?:read_attr|attr_u32|attr_f64|attr)\s*\(\s*&?\w+\s*,\s*b"([\w:.-]+)"')


def importer_surface(src_dir):
    """{element: {attributes read inside that element's match arm}}.

    Scoped by brace matching from `b"name" =>` so an attribute read under one
    element is not credited to another.
    """
    surface = defaultdict(set)
    for path in sorted(pathlib.Path(src_dir).glob("*.rs")):
        src = path.read_text()
        for m in re.finditer(r'b"([\w:.-]+)"\s*(?:\|[^=]*)?=>', src):
            name = m.group(1)
            i = src.find("{", m.end())
            arm_end = src.find("\n", m.end())
            if i == -1 or (arm_end != -1 and i > arm_end + 200):
                surface[name]  # an arm with no block still registers the element
                continue
            depth, j = 0, i
            while j < len(src):
                if src[j] == "{":
                    depth += 1
                elif src[j] == "}":
                    depth -= 1
                    if depth == 0:
                        break
                j += 1
            for a in ATTR_READ.finditer(src[i:j]):
                surface[name].add(a.group(1))
            surface[name]
    return surface


def exporter_surface(src_dir):
    """{element: {attributes written}} from the literal tags the writer emits."""
    surface = defaultdict(set)
    for path in sorted(pathlib.Path(src_dir).glob("*.rs")):
        if path.name == "tests.rs":
            continue
        src = path.read_text()
        for m in re.finditer(r"<([A-Za-z][\w:.-]*)", src):
            name = m.group(1).split(":")[-1]
            tail = src[m.end(): m.end() + 900]
            cut = re.search(r"/?\\?\">|</", tail)
            span = tail[: cut.start()] if cut else tail
            surface[name] |= set(re.findall(r'([\w:.-]+)=\\"', span))
            surface[name]
    return surface


def main():
    schema = ROOT / "schemas/ooxml/sml.xsd"
    parts = walk(schema)
    imp = importer_surface(ROOT / "crates/casual-calc-import/src")
    exp = exporter_surface(ROOT / "crates/casual-calc-export/src")

    rows = []
    for part, elems in parts.items():
        for elem, attrs in sorted(elems.items()):
            read, written = elem in imp, elem in exp
            for attr in attrs:
                rows.append({
                    "part": part, "element": elem, "attribute": attr,
                    "read": attr in imp.get(elem, ()),
                    "written": attr in exp.get(elem, ()),
                })
            if not attrs:
                rows.append({"part": part, "element": elem, "attribute": None,
                             "read": read, "written": written})
    json.dump(rows, sys.stdout, indent=1)


if __name__ == "__main__":
    main()
