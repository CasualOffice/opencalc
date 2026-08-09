"""Shared schema reader: element -> attributes, with sample values by type."""
import pathlib, re, xml.etree.ElementTree as ET

XS = "{http://www.w3.org/2001/XMLSchema}"
ROOT = pathlib.Path(__file__).resolve().parents[2]
SML = ROOT / "schemas/ooxml/sml.xsd"


class Schema:
    def __init__(self, path=SML):
        r = ET.parse(path).getroot()
        self.ctypes, self.groups, self.agroups, self.stypes, self.top = {}, {}, {}, {}, {}
        for ct in r.findall(f"{XS}complexType"):
            if ct.get("name"): self.ctypes[ct.get("name")] = ct
        for g in r.findall(f"{XS}group"):
            if g.get("name"): self.groups[g.get("name")] = g
        for g in r.findall(f"{XS}attributeGroup"):
            if g.get("name"): self.agroups[g.get("name")] = g
        for st in r.findall(f"{XS}simpleType"):
            if st.get("name"): self.stypes[st.get("name")] = st
        for e in r.findall(f"{XS}element"):
            if e.get("name"): self.top.setdefault(e.get("name"), e.get("type"))
        # element name -> complexType, discovered by walking every type
        self.elem_type = {}
        for name, ct in self.ctypes.items():
            for cn, ctyp in self.children(ct):
                if ctyp: self.elem_type.setdefault(cn, ctyp)
        for n, t in self.top.items():
            if t: self.elem_type.setdefault(n, t)

    def children(self, node, seen=frozenset()):
        out = []
        for ch in node:
            tag = ch.tag.replace(XS, "")
            if tag == "element":
                ref = ch.get("ref")
                name = ch.get("name") or (ref.split(":")[-1] if ref else None)
                typ = ch.get("type") or (self.top.get(name) if ref else None)
                if name: out.append((name, typ))
            elif tag in ("sequence", "choice", "all", "complexContent", "simpleContent"):
                out += self.children(ch, seen)
            elif tag == "group":
                ref = (ch.get("ref") or "").split(":")[-1]
                if ref in self.groups and ref not in seen:
                    out += self.children(self.groups[ref], seen | {ref})
            elif tag == "extension":
                base = (ch.get("base") or "").split(":")[-1]
                if base in self.ctypes: out += self.children(self.ctypes[base], seen)
                out += self.children(ch, seen)
        return out

    def attrs(self, node, seen=frozenset()):
        """[(name, type)] declared on a complexType, following attributeGroups."""
        out = []
        for ch in node:
            tag = ch.tag.replace(XS, "")
            if tag == "attribute":
                n = ch.get("name") or (ch.get("ref") or "").split(":")[-1]
                # The declared default matters as much as the type: a writer is
                # *correct* to omit an attribute whose value is the default, so
                # a probe that supplies the default cannot tell preservation
                # from omission and scores working code as broken.
                if n: out.append((n, (ch.get("type") or "").split(":")[-1], ch.get("default")))
            elif tag == "attributeGroup":
                ref = (ch.get("ref") or "").split(":")[-1]
                if ref in self.agroups and ref not in seen:
                    out += self.attrs(self.agroups[ref], seen | {ref})
            elif tag in ("complexContent", "simpleContent"):
                out += self.attrs(ch, seen)
            elif tag == "extension":
                base = (ch.get("base") or "").split(":")[-1]
                if base in self.ctypes: out += self.attrs(self.ctypes[base], seen)
                out += self.attrs(ch, seen)
        return out

    def attrs_of_element(self, name):
        t = self.elem_type.get(name)
        return self.attrs(self.ctypes[t]) if t in self.ctypes else []

    def enum_values(self, tname):
        st = self.stypes.get(tname)
        if st is None: return []
        return [e.get("value") for e in st.iter(f"{XS}enumeration")]
