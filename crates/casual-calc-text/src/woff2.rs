//! WOFF2 → sfnt, for the **null-transform** subset of WOFF2 (`XL-FEAT-140`).
//!
//! The CJK slices the editor serves are WOFF2 so a browser downloads them
//! small, and the exporters, which run in Rust and in WebAssembly, are handed
//! the **same files** by the host rather than a second copy in another format.
//! So this crate has to read WOFF2.
//!
//! Full WOFF2 has a `glyf`/`loca` transform whose reconstruction is a few
//! hundred lines of triplet decoding. The slices are written by
//! `tools/fonts/build-cjk-slices.py` with that transform **off** (WOFF2 §5.1,
//! transform version 3), which leaves a decoder that is: read a table
//! directory, brotli-decompress one stream, put the tables back into an sfnt.
//! A file that does use the transform is refused by name rather than
//! misread — that is a font from somewhere else, and [`register_face`]
//! accepts TTF/OTF for those.
//!
//! # Bounds
//!
//! The bytes come from a host, and a host can be pointed anywhere, so the
//! decoder is bounded like every parser here: the decompressed stream may not
//! exceed `max_out` (checked while decompressing, not after), the table count
//! is capped, and every length is checked against the stream it indexes.
//!
//! [`register_face`]: crate::faces::register_face

use std::io::Read as _;

/// Why a WOFF2 file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Woff2Error {
    /// Not WOFF2, or truncated before its own header ends.
    NotWoff2,
    /// A font collection; the slices are single faces.
    Collection,
    /// `glyf`/`loca` (or `hmtx`) use the WOFF2 transform this decoder does not
    /// implement.
    Transformed,
    /// A length, offset or count that does not fit the file.
    Malformed(&'static str),
    /// The decompressed stream would exceed the caller's cap.
    TooLarge,
}

impl std::fmt::Display for Woff2Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotWoff2 => f.write_str("not a WOFF2 file"),
            Self::Collection => f.write_str("WOFF2 font collections are not supported"),
            Self::Transformed => f.write_str(
                "WOFF2 with transformed glyf/loca/hmtx is not supported (rebuild with the null transform)",
            ),
            Self::Malformed(what) => write!(f, "malformed WOFF2: {what}"),
            Self::TooLarge => f.write_str("WOFF2 decompresses past the size cap"),
        }
    }
}

impl std::error::Error for Woff2Error {}

/// Whether `bytes` start with the WOFF2 signature.
#[must_use]
pub fn is_woff2(bytes: &[u8]) -> bool {
    bytes.starts_with(b"wOF2")
}

/// The 63 tags WOFF2 abbreviates to an index (WOFF2 §5.1, table 1).
const KNOWN_TAGS: [&[u8; 4]; 63] = [
    b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"OS/2", b"post", b"cvt ", b"fpgm",
    b"glyf", b"loca", b"prep", b"CFF ", b"VORG", b"EBDT", b"EBLC", b"gasp", b"hdmx", b"kern",
    b"LTSH", b"PCLT", b"VDMX", b"vhea", b"vmtx", b"BASE", b"GDEF", b"GPOS", b"GSUB", b"EBSC",
    b"JSTF", b"MATH", b"CBDT", b"CBLC", b"COLR", b"CPAL", b"SVG ", b"sbix", b"acnt", b"avar",
    b"bdat", b"bloc", b"bsln", b"cvar", b"fdsc", b"feat", b"fmtx", b"fvar", b"gvar", b"hsty",
    b"just", b"lcar", b"mort", b"morx", b"opbd", b"prop", b"trak", b"Zapf", b"Silf", b"Glat",
    b"Gloc", b"Feat", b"Sill",
];

/// More tables than any real face carries; a directory claiming more is
/// either hostile or broken.
const MAX_TABLES: usize = 64;

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn u8(&mut self) -> Result<u8, Woff2Error> {
        let b = *self
            .bytes
            .get(self.at)
            .ok_or(Woff2Error::Malformed("truncated"))?;
        self.at += 1;
        Ok(b)
    }
    fn u16(&mut self) -> Result<u16, Woff2Error> {
        Ok(u16::from_be_bytes([self.u8()?, self.u8()?]))
    }
    fn u32(&mut self) -> Result<u32, Woff2Error> {
        Ok(u32::from_be_bytes([
            self.u8()?,
            self.u8()?,
            self.u8()?,
            self.u8()?,
        ]))
    }
    /// `UIntBase128` (WOFF2 §4.1): at most five bytes, no leading zero byte,
    /// no overflow past 32 bits.
    fn base128(&mut self) -> Result<u32, Woff2Error> {
        let mut value: u32 = 0;
        for i in 0..5 {
            let b = self.u8()?;
            if i == 0 && b == 0x80 {
                return Err(Woff2Error::Malformed("UIntBase128 leading zero"));
            }
            if value & 0xFE00_0000 != 0 {
                return Err(Woff2Error::Malformed("UIntBase128 overflow"));
            }
            value = (value << 7) | u32::from(b & 0x7F);
            if b & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(Woff2Error::Malformed("UIntBase128 longer than five bytes"))
    }
}

/// Decode a null-transform WOFF2 file into the sfnt it wraps.
///
/// # Errors
///
/// See [`Woff2Error`]. Nothing is returned partially: a refused file yields
/// no bytes at all.
pub fn to_sfnt(bytes: &[u8], max_out: usize) -> Result<Vec<u8>, Woff2Error> {
    if !is_woff2(bytes) || bytes.len() < 48 {
        return Err(Woff2Error::NotWoff2);
    }
    let mut c = Cursor { bytes, at: 4 };
    let flavor = c.u32()?;
    if flavor == u32::from_be_bytes(*b"ttcf") {
        return Err(Woff2Error::Collection);
    }
    let _length = c.u32()?;
    let num_tables = usize::from(c.u16()?);
    let _reserved = c.u16()?;
    let total_sfnt = c.u32()? as usize;
    let compressed_len = c.u32()? as usize;
    // major/minor version, then metadata and private blocks: not needed.
    c.at = 48;
    if num_tables == 0 || num_tables > MAX_TABLES {
        return Err(Woff2Error::Malformed("table count"));
    }
    if total_sfnt > max_out {
        return Err(Woff2Error::TooLarge);
    }

    let mut tables: Vec<([u8; 4], usize)> = Vec::with_capacity(num_tables);
    for _ in 0..num_tables {
        let flags = c.u8()?;
        let tag: [u8; 4] = match flags & 0x3F {
            63 => c.u32()?.to_be_bytes(),
            i => *KNOWN_TAGS[usize::from(i)],
        };
        let version = flags >> 6;
        let orig = c.base128()? as usize;
        // For glyf and loca the *null* transform is version 3; for every
        // other table it is version 0. Anything else carries a transformLength
        // and data this decoder cannot rebuild.
        let null = if &tag == b"glyf" || &tag == b"loca" {
            version == 3
        } else {
            version == 0
        };
        if !null {
            return Err(Woff2Error::Transformed);
        }
        tables.push((tag, orig));
    }

    let start = c.at;
    let end = start
        .checked_add(compressed_len)
        .filter(|&e| e <= bytes.len())
        .ok_or(Woff2Error::Malformed("compressed stream past end of file"))?;
    let expected: usize = tables.iter().map(|(_, n)| *n).sum();
    if expected > max_out {
        return Err(Woff2Error::TooLarge);
    }
    let mut stream = Vec::with_capacity(expected);
    let reader = brotli_decompressor::Decompressor::new(&bytes[start..end], 4096);
    // One byte past the expectation, so a stream that runs long is caught
    // rather than silently truncated to fit.
    reader
        .take(expected as u64 + 1)
        .read_to_end(&mut stream)
        .map_err(|_| Woff2Error::Malformed("brotli stream"))?;
    if stream.len() != expected {
        return Err(Woff2Error::Malformed(
            "decompressed size disagrees with the directory",
        ));
    }

    // Reassemble: header, records sorted by tag, 4-byte aligned bodies.
    let mut order: Vec<usize> = (0..tables.len()).collect();
    order.sort_by_key(|&i| tables[i].0);
    let mut offsets = vec![0usize; tables.len()];
    let mut at = 0usize;
    for (i, (_, len)) in tables.iter().enumerate() {
        offsets[i] = at;
        at += len;
    }
    let n = tables.len();
    let header = 12 + 16 * n;
    let mut out = Vec::with_capacity(header + expected + 4 * n);
    let entry_selector = (usize::BITS - 1 - n.leading_zeros()) as u16;
    let search_range = (1u16 << entry_selector) * 16;
    out.extend_from_slice(&flavor.to_be_bytes());
    out.extend_from_slice(&(n as u16).to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&((n as u16) * 16 - search_range).to_be_bytes());
    let mut records = Vec::with_capacity(16 * n);
    let mut body: Vec<u8> = Vec::with_capacity(expected + 4 * n);
    let mut head_at = None;
    for &i in &order {
        let (tag, len) = tables[i];
        let data = &stream[offsets[i]..offsets[i] + len];
        let offset = header + body.len();
        if &tag == b"head" {
            head_at = Some(offset);
        }
        records.extend_from_slice(&tag);
        records.extend_from_slice(&checksum(data).to_be_bytes());
        records.extend_from_slice(&(offset as u32).to_be_bytes());
        records.extend_from_slice(&(len as u32).to_be_bytes());
        body.extend_from_slice(data);
        while !body.len().is_multiple_of(4) {
            body.push(0);
        }
    }
    out.extend_from_slice(&records);
    out.extend_from_slice(&body);
    if let Some(h) = head_at
        && out.len() >= h + 12
    {
        out[h + 8..h + 12].copy_from_slice(&[0; 4]);
        let adjust = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
        out[h + 8..h + 12].copy_from_slice(&adjust.to_be_bytes());
    }
    Ok(out)
}

/// The sfnt table checksum: the big-endian `u32` sum, zero-padded.
pub(crate) fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice(path: &str) -> Vec<u8> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../webapp/fonts/cjk")
            .join(path);
        std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    }

    #[test]
    fn a_shipped_slice_decodes_to_a_face_skrifa_reads() {
        use skrifa::MetadataProvider as _;
        let sfnt = to_sfnt(&slice("jp-sans-400/000.woff2"), 1 << 24).expect("decodes");
        let font = skrifa::FontRef::new(&sfnt).expect("an sfnt skrifa accepts");
        assert!(
            font.charmap().mappings().count() > 50,
            "the slice maps its characters"
        );
    }

    #[test]
    fn truncation_and_caps_are_refused_not_misread() {
        let bytes = slice("jp-sans-400/000.woff2");
        assert_eq!(to_sfnt(&bytes[..40], 1 << 24), Err(Woff2Error::NotWoff2));
        assert!(to_sfnt(&bytes[..bytes.len() / 2], 1 << 24).is_err());
        assert_eq!(to_sfnt(&bytes, 1000), Err(Woff2Error::TooLarge));
        assert_eq!(
            to_sfnt(
                b"\0\x01\0\0 not woff2 at all, padding padding padding",
                1 << 24
            ),
            Err(Woff2Error::NotWoff2)
        );
    }

    #[test]
    fn a_transformed_glyf_is_refused_by_name() {
        // Flip the first table's transform bits to version 0 on a glyf entry by
        // building a minimal directory: one glyf table, transform version 0.
        let mut f = b"wOF2\0\x01\0\0".to_vec();
        f.extend_from_slice(&[0u8; 40]);
        f[12..14].copy_from_slice(&1u16.to_be_bytes());
        f.push(10); // glyf, version 0 => transformed
        f.push(4);
        f.push(4);
        assert_eq!(to_sfnt(&f, 1 << 20), Err(Woff2Error::Transformed));
    }
}
