//! A TrueType subsetter for PDF embedding: keep the drawn glyphs, empty the
//! rest (`XL-FEAT-140`).
//!
//! # Why this shape of subset
//!
//! A PDF's `CIDFontType2` with `/CIDToGIDMap /Identity` addresses glyphs by
//! their id in the embedded program, and the content streams, `/W` and
//! `ToUnicode` are already written in those ids. So the cheapest correct
//! subset **keeps every glyph id where it is** and empties the outlines of the
//! ones not drawn: `glyf` shrinks to what the page uses, `loca` is rewritten
//! to point at it, and nothing else in the document has to be renumbered.
//! Composite glyphs pull in their components, transitively.
//!
//! Only the tables a PDF consumer reads from an embedded TrueType program are
//! kept (ISO 32000-1 §9.9: `head`, `hhea`, `loca`, `maxp`, `cvt `, `prep`,
//! `glyf`, `hmtx`, `fpgm`), plus `OS/2` and `post`, which some viewers consult
//! when repairing a font. No `cmap`: Identity addressing does not use one.
//!
//! A face that is not TrueType-flavoured (`OTTO`/CFF, a collection) is not
//! subset; the caller embeds it as it was, which is what happened to every
//! face before this existed.

use std::collections::{BTreeMap, BTreeSet};

const KEEP: [&[u8; 4]; 11] = [
    b"OS/2", b"cvt ", b"fpgm", b"glyf", b"head", b"hhea", b"hmtx", b"loca", b"maxp", b"post",
    b"prep",
];

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *b.get(at)?,
        *b.get(at + 1)?,
        *b.get(at + 2)?,
        *b.get(at + 3)?,
    ]))
}

/// The tables of an sfnt, by tag.
fn tables(font: &[u8]) -> Option<BTreeMap<[u8; 4], &[u8]>> {
    let version = u32_at(font, 0)?;
    if version != 0x0001_0000 && version != u32::from_be_bytes(*b"true") {
        return None;
    }
    let n = usize::from(u16_at(font, 4)?);
    let mut out = BTreeMap::new();
    for i in 0..n {
        let rec = 12 + 16 * i;
        let tag: [u8; 4] = font.get(rec..rec + 4)?.try_into().ok()?;
        let offset = u32_at(font, rec + 8)? as usize;
        let length = u32_at(font, rec + 12)? as usize;
        out.insert(tag, font.get(offset..offset.checked_add(length)?)?);
    }
    Some(out)
}

/// The glyph ids a composite glyph's components name.
fn components(glyph: &[u8]) -> Vec<u16> {
    let mut out = Vec::new();
    if glyph.len() < 10 || i16::from_be_bytes([glyph[0], glyph[1]]) >= 0 {
        return out;
    }
    let mut at = 10;
    // A composite of more components than bytes is malformed; the bound is
    // the glyph's own length.
    while let (Some(flags), Some(gid)) = (u16_at(glyph, at), u16_at(glyph, at + 2)) {
        out.push(gid);
        at += 4;
        at += if flags & 0x0001 != 0 { 4 } else { 2 };
        if flags & 0x0008 != 0 {
            at += 2;
        } else if flags & 0x0040 != 0 {
            at += 4;
        } else if flags & 0x0080 != 0 {
            at += 8;
        }
        if flags & 0x0020 == 0 || at >= glyph.len() {
            break;
        }
    }
    out
}

/// `font` with every glyph outside `used` (and their components) emptied, or
/// `None` when the face is not one this can subset.
#[must_use]
pub fn subset(font: &[u8], used: impl IntoIterator<Item = u32>) -> Option<Vec<u8>> {
    let t = tables(font)?;
    let head = *t.get(b"head")?;
    let maxp = *t.get(b"maxp")?;
    let loca = *t.get(b"loca")?;
    let glyf = *t.get(b"glyf")?;
    t.get(b"hhea")?;
    t.get(b"hmtx")?;
    let long = i16::from_be_bytes([*head.get(50)?, *head.get(51)?]) != 0;
    let num_glyphs = usize::from(u16_at(maxp, 4)?);
    let span = |gid: usize| -> Option<&[u8]> {
        let (a, b) = if long {
            (
                u32_at(loca, gid * 4)? as usize,
                u32_at(loca, gid * 4 + 4)? as usize,
            )
        } else {
            (
                usize::from(u16_at(loca, gid * 2)?) * 2,
                usize::from(u16_at(loca, gid * 2 + 2)?) * 2,
            )
        };
        if a > b {
            return None;
        }
        glyf.get(a..b)
    };

    // The closure over composites: a worklist with a visited set, so a cycle
    // in a malformed face terminates.
    let mut keep: BTreeSet<usize> = BTreeSet::from([0]);
    let mut work: Vec<usize> = used
        .into_iter()
        .map(|g| g as usize)
        .filter(|&g| g < num_glyphs)
        .collect();
    while let Some(gid) = work.pop() {
        if !keep.insert(gid) && gid != 0 {
            continue;
        }
        for c in components(span(gid)?) {
            let c = usize::from(c);
            if c < num_glyphs && !keep.contains(&c) {
                work.push(c);
            }
        }
    }

    let mut new_glyf = Vec::new();
    let mut new_loca = Vec::with_capacity((num_glyphs + 1) * 4);
    for gid in 0..num_glyphs {
        new_loca.extend_from_slice(&u32::try_from(new_glyf.len()).ok()?.to_be_bytes());
        if keep.contains(&gid) {
            new_glyf.extend_from_slice(span(gid)?);
            while !new_glyf.len().is_multiple_of(4) {
                new_glyf.push(0);
            }
        }
    }
    new_loca.extend_from_slice(&u32::try_from(new_glyf.len()).ok()?.to_be_bytes());
    let mut new_head = head.to_vec();
    new_head[50..52].copy_from_slice(&1i16.to_be_bytes());

    let mut out_tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    for tag in KEEP {
        let data = match tag {
            b"glyf" => new_glyf.clone(),
            b"loca" => new_loca.clone(),
            b"head" => new_head.clone(),
            _ => match t.get(tag) {
                Some(d) => d.to_vec(),
                None => continue,
            },
        };
        out_tables.push((*tag, data));
    }
    Some(assemble(&out_tables))
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// An sfnt from tables already in tag order.
fn assemble(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let n = tables.len();
    let header = 12 + 16 * n;
    let entry_selector = (usize::BITS - 1 - n.leading_zeros()) as u16;
    let search_range = (1u16 << entry_selector) * 16;
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(n as u16).to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&((n as u16) * 16 - search_range).to_be_bytes());
    let mut body = Vec::new();
    let mut head_at = None;
    for (tag, data) in tables {
        let offset = header + body.len();
        if tag == b"head" {
            head_at = Some(offset);
        }
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        while !body.len().is_multiple_of(4) {
            body.push(0);
        }
    }
    out.extend_from_slice(&body);
    if let Some(h) = head_at {
        out[h + 8..h + 12].copy_from_slice(&[0; 4]);
        let adjust = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
        out[h + 8..h + 12].copy_from_slice(&adjust.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use skrifa::{FontRef, MetadataProvider};

    #[test]
    fn a_subset_keeps_the_drawn_glyphs_at_their_ids_and_is_much_smaller() {
        let face = casual_calc_text::faces::face_bytes_for(Some("Calibri"), false, false);
        let font = FontRef::new(face).unwrap();
        let ids: Vec<u32> = "Total"
            .chars()
            .map(|c| font.charmap().map(c).unwrap().to_u32())
            .collect();
        let small = subset(face, ids.iter().copied()).expect("a TrueType face subsets");
        assert!(
            small.len() * 4 < face.len(),
            "{} of {} bytes",
            small.len(),
            face.len()
        );
        let sub = FontRef::new(&small).expect("the subset is a face skrifa reads");
        let outlines = sub.outline_glyphs();
        let full = font.outline_glyphs();
        for &id in &ids {
            let g = skrifa::GlyphId::new(id);
            let mut a = Count(0);
            let mut b = Count(0);
            let settings = || {
                skrifa::outline::DrawSettings::unhinted(
                    skrifa::instance::Size::new(20.0),
                    skrifa::instance::LocationRef::default(),
                )
            };
            outlines.get(g).unwrap().draw(settings(), &mut a).unwrap();
            full.get(g).unwrap().draw(settings(), &mut b).unwrap();
            assert_eq!(a.0, b.0, "glyph {id} keeps its outline at the same id");
            assert!(a.0 > 0);
        }
        // A glyph that was not drawn is empty.
        let other = font.charmap().map('Q').unwrap();
        let mut q = Count(0);
        if let Some(g) = outlines.get(other) {
            let _ = g.draw(
                skrifa::outline::DrawSettings::unhinted(
                    skrifa::instance::Size::new(20.0),
                    skrifa::instance::LocationRef::default(),
                ),
                &mut q,
            );
        }
        assert_eq!(q.0, 0, "an undrawn glyph has no outline left");
    }

    #[test]
    fn hostile_input_is_refused_not_panicked_on() {
        assert!(subset(b"", [1]).is_none());
        assert!(subset(b"OTTO\0\0\0\0\0\0\0\0", [1]).is_none());
        let mut bad = vec![0, 1, 0, 0, 0, 200];
        bad.resize(64, 0xFF);
        assert!(subset(&bad, [1]).is_none());
        // A composite naming itself terminates.
        assert_eq!(
            components(&[0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x20, 0, 5]).len(),
            1
        );
    }

    struct Count(usize);
    impl skrifa::outline::OutlinePen for Count {
        fn move_to(&mut self, _: f32, _: f32) {
            self.0 += 1;
        }
        fn line_to(&mut self, _: f32, _: f32) {
            self.0 += 1;
        }
        fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
            self.0 += 1;
        }
        fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
            self.0 += 1;
        }
        fn close(&mut self) {}
    }
}
