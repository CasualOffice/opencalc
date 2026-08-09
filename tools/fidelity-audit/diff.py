#!/usr/bin/env python3
"""Compare the XML that went in against the XML that came back out.

Reports every (element, attribute) present in the source package but absent
from the written one. This is the only measurement in the audit that cannot be
fooled by how the code is written: it observes bytes in and bytes out, not the
importer's or exporter's source, both of which reach attributes through helper
functions and interpolated fragments that no static scan can follow.
"""
import collections, sys, zipfile
import xml.etree.ElementTree as ET

def surface(package):
    """{(element, attribute)} and {element} present anywhere in the package."""
    elems, pairs = set(), set()
    with zipfile.ZipFile(package) as z:
        for name in z.namelist():
            if not name.endswith(".xml") or name.startswith("_rels") or "/_rels/" in name:
                continue
            try:
                root = ET.fromstring(z.read(name))
            except ET.ParseError:
                continue
            for node in root.iter():
                tag = node.tag.split("}")[-1]
                elems.add(tag)
                for a in node.attrib:
                    pairs.add((tag, a.split("}")[-1]))
    return elems, pairs

src_e, src_p = surface(sys.argv[1])
out_e, out_p = surface(sys.argv[2])

lost_e = sorted(src_e - out_e)
lost_p = sorted(p for p in src_p - out_p if p[0] not in lost_e)

print(f"elements  in={len(src_e):>4}  out={len(out_e):>4}  LOST={len(lost_e)}")
print(f"attributes in={len(src_p):>4}  out={len(out_p):>4}  LOST(on kept elements)={len(lost_p)}")
print("\n=== ELEMENTS DROPPED ENTIRELY ===")
print(", ".join(lost_e) or "(none)")
print("\n=== ATTRIBUTES DROPPED (element survived, attribute did not) ===")
by = collections.defaultdict(list)
for el, at in lost_p: by[el].append(at)
for el in sorted(by): print(f"  {el:<18} {', '.join(sorted(by[el]))}")
