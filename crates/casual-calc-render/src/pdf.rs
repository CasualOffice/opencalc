//! The PDF backend: a second renderer over the same display list (`IO-03`).
//!
//! # Why the bytes are written here, by hand
//!
//! A PDF is a very small graphics language wrapped in an object store with a
//! byte-offset index. Everything this needs from it — rectangles, lines, cubic
//! curves, a colour, a clip, and a Type0 font over an embedded TrueType face —
//! is a few dozen operators. Against that, every PDF crate on the registry
//! carries a compression stack, an image pipeline and usually a font subsetter,
//! which is a large new surface reached by a **workbook's own bytes** (font
//! names, colours, cell strings) in a workspace that forbids `unsafe_code` and
//! counts its dependencies one at a time. So this adds **no dependency at all**:
//! it uses `skrifa`, which is already here for the raster backend, for the
//! metrics and glyph ids, and writes the file itself.
//!
//! The cost of that choice is stated rather than hidden, and it is the font: a
//! face is embedded **whole**, not subset, so a page with one line of Carlito
//! carries the 628 KB face. Subsetting is a real piece of work (`glyf`/`loca`
//! rewriting and a `cmap` rebuild) and is deliberately not in this increment.
//!
//! # Why it is a backend and not an exporter
//!
//! It consumes [`DisplayList`]s and knows nothing about workbooks, sheets or
//! print settings. Pagination is arithmetic over the grid and lives in
//! [`casual_calc_layout::print`]; composing the pages is the SDK's job, exactly
//! as it is for the frozen panes of the PNG path. That division is what keeps
//! the PNG and the PDF showing the same picture: both are executions of the
//! same list, and a difference between them can only be a bug in one executor.
//!
//! # Coordinates
//!
//! PDF space is points (1/72"), y **up** from the bottom-left. The display list
//! is twips, y **down** from the top-left. Rather than convert every number,
//! each page installs one matrix — `s/20 0 0 -s/20 mx (h-my) cm` — so
//! everything below it is written in **content twips, y down**, and the only
//! place the flip is thought about twice is text, where a `1 0 0 -1 x y Tm`
//! text matrix flips the glyphs back upright.
//!
//! # Determinism
//!
//! No creation date, no file id, no producer version: the same workbook gives
//! the same bytes. Faces are numbered in first-encounter order over the pages,
//! which is deterministic because the display list is.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use casual_calc_layout::{
    Align, BorderLine, DisplayList, GridGeometry, PaintItem, Point as LayoutPoint,
    Rect as LayoutRect,
};
use casual_calc_text::faces as fonts;
use skrifa::instance::{LocationRef, Size};
use skrifa::{FontRef, MetadataProvider};

use crate::RenderError;
use crate::images::{ImageReport, UndrawnReason};

/// Twips in one PDF point.
const TWIPS_PER_POINT: f64 = 20.0;

/// Twips in one display-list pixel. [`BorderLine::width`] is quoted in pixels
/// at the 96 dpi the layout crate resolves border styles against.
const TWIPS_PER_PX: i64 = 15;

/// The gridline grey, matching [`crate::draw_gridlines`]'s `224,224,224`. The
/// two backends draw one grid; a print-only shade would make a PNG and a PDF of
/// the same sheet disagree for no reason anybody could see from the file.
const GRID_GREY: f64 = 224.0 / 255.0;

/// Gridline and merge-outline stroke width, in content twips: half a point.
const HAIRLINE_TWIPS: f64 = 10.0;

/// The pad a cell's text is inset by, matching the raster backend.
const TEXT_PAD_TWIPS: f64 = 30.0;

/// The point size a [`PaintItem::Text`] with no size of its own is drawn at,
/// matching the raster backend.
const DEFAULT_FONT_PT: f32 = 11.0;

/// How opaque a conditional-formatting data bar is painted. The number is still
/// read through it, which is the whole point of a data bar.
const DATA_BAR_ALPHA: f64 = 0.55;

/// One band of a page: a display list placed at a known spot on the paper.
///
/// A page is up to four of these, exactly as a frozen sheet is up to four
/// panes: the repeated title columns, the repeated title rows, the corner where
/// they cross, and the body. Each is laid out over its own row and column band
/// and then **moved** so the repeated lines sit against the margin and the body
/// starts after them.
#[derive(Debug, Clone, Copy)]
pub struct PdfBand<'a> {
    /// What to paint. Its items are in sheet coordinates.
    pub display_list: &'a DisplayList,
    /// First and last row of the band, inclusive.
    pub rows: (u32, u32),
    /// First and last column of the band, inclusive.
    pub cols: (u32, u32),
    /// Where the band's top-left goes, in content twips (y down from the
    /// printable box's top-left corner).
    pub origin: (i64, i64),
    /// Whether to lay gridlines under the items.
    pub gridlines: bool,
}

/// One sheet of paper.
#[derive(Debug, Clone)]
pub struct PdfPage<'a> {
    /// Paper width in twips, already turned for the orientation.
    pub width: i64,
    /// Paper height in twips, already turned for the orientation.
    pub height: i64,
    /// Left margin in twips.
    pub margin_left: i64,
    /// Top margin in twips.
    pub margin_top: i64,
    /// The scale the content is drawn at, as a fraction.
    pub scale: f64,
    /// The bands, in painter's order.
    pub bands: Vec<PdfBand<'a>>,
}

/// What the document says about itself.
#[derive(Debug, Clone, Default)]
pub struct PdfMetadata {
    /// The document title. Empty writes no `/Info` at all.
    pub title: String,
}

/// A face the document embeds, and the glyphs it was asked for.
struct Face {
    bytes: &'static [u8],
    /// Glyph id → the first character that mapped to it, for `ToUnicode`.
    used: BTreeMap<u32, char>,
}

/// The faces a document uses, in first-encounter order.
///
/// Ordered by encounter rather than by anything about the face itself: a
/// `BTreeMap` keyed on the byte slice's address would order the fonts
/// differently on every run, and this workspace's second engineering priority
/// is that the same input gives the same bytes.
#[derive(Default)]
struct Faces {
    faces: Vec<Face>,
}

impl Faces {
    /// The resource index for a face, inserting it if it is new.
    fn index(&mut self, bytes: &'static [u8]) -> usize {
        if let Some(i) = self.faces.iter().position(|f| {
            std::ptr::eq(f.bytes.as_ptr(), bytes.as_ptr()) && f.bytes.len() == bytes.len()
        }) {
            return i;
        }
        self.faces.push(Face {
            bytes,
            used: BTreeMap::new(),
        });
        self.faces.len() - 1
    }

    /// Record that a glyph was drawn, so it reaches `/W` and `ToUnicode`.
    fn note(&mut self, index: usize, gid: u32, ch: char) {
        if let Some(face) = self.faces.get_mut(index) {
            face.used.entry(gid).or_insert(ch);
        }
    }
}

/// Write a PDF of `pages`.
///
/// The report names the pictures that did not reach the paper, on the same
/// terms the PNG path uses: nothing is dropped without being counted.
///
/// # Errors
///
/// [`RenderError::InvalidSize`] when a page has no area — a media box of zero
/// is a file no viewer will open, and silently substituting Letter would hide
/// whatever produced the zero.
pub fn write_pdf(
    pages: &[PdfPage<'_>],
    geometry: &GridGeometry,
    meta: &PdfMetadata,
) -> Result<(Vec<u8>, ImageReport), RenderError> {
    for page in pages {
        if page.width <= 0 || page.height <= 0 {
            return Err(RenderError::InvalidSize {
                width: u32::try_from(page.width.max(0)).unwrap_or(u32::MAX),
                height: u32::try_from(page.height.max(0)).unwrap_or(u32::MAX),
            });
        }
    }

    // Content first: it is what discovers which faces the document needs, and
    // a face cannot be written before it is known.
    let mut faces = Faces::default();
    let mut report = ImageReport::default();
    let mut streams: Vec<String> = Vec::with_capacity(pages.len());
    for page in pages {
        streams.push(page_content(page, geometry, &mut faces, &mut report));
    }

    let mut objects = Objects::default();
    let catalog = objects.reserve();
    let pages_id = objects.reserve();

    // One resource dictionary shared by every page: the faces are collected
    // across the whole document, so a per-page dictionary would list the same
    // objects again for every sheet of paper.
    let font_ids = write_faces(&mut objects, &faces);
    let alpha = objects
        .set_now(format!("<< /Type /ExtGState /ca {} /CA 1 >>", num(DATA_BAR_ALPHA)).into_bytes());
    let mut resources = String::from("<< /ExtGState << /GSa ");
    let _ = write!(resources, "{alpha} 0 R >> /Font << ");
    for (i, id) in font_ids.iter().enumerate() {
        let _ = write!(resources, "/F{i} {id} 0 R ");
    }
    resources.push_str(">> >>");
    let resources_id = objects.set_now(resources.into_bytes());

    let mut kids = String::new();
    for (page, stream) in pages.iter().zip(&streams) {
        let content_id = objects.stream(b"", stream.as_bytes());
        let mut body = String::from("<< /Type /Page /Parent ");
        let _ = write!(body, "{pages_id} 0 R /MediaBox [0 0 ");
        let _ = write!(
            body,
            "{} {}] /Resources {resources_id} 0 R /Contents {content_id} 0 R >>",
            num(page.width as f64 / TWIPS_PER_POINT),
            num(page.height as f64 / TWIPS_PER_POINT),
        );
        let id = objects.set_now(body.into_bytes());
        let _ = write!(kids, "{id} 0 R ");
    }

    objects.set(
        pages_id,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.trim_end(),
            pages.len()
        )
        .into_bytes(),
    );

    let info = (!meta.title.is_empty())
        .then(|| objects.set_now(format!("<< /Title {} >>", pdf_string(&meta.title)).into_bytes()));

    objects.set(
        catalog,
        format!("<< /Type /Catalog /Pages {pages_id} 0 R >>").into_bytes(),
    );

    Ok((objects.finish(catalog, info), report))
}

// ---------------------------------------------------------------------------
// The object store
// ---------------------------------------------------------------------------

/// Indirect objects by number, written out with a cross-reference table.
///
/// Numbers can be **reserved** before their body is known, because a PDF is a
/// graph and not a list: the catalog points at the page tree, the page tree at
/// the pages, and every page back at the tree.
#[derive(Default)]
struct Objects {
    bodies: Vec<Option<Vec<u8>>>,
}

impl Objects {
    fn reserve(&mut self) -> usize {
        self.bodies.push(None);
        self.bodies.len()
    }

    fn set(&mut self, id: usize, body: Vec<u8>) {
        self.bodies[id - 1] = Some(body);
    }

    fn set_now(&mut self, body: Vec<u8>) -> usize {
        self.bodies.push(Some(body));
        self.bodies.len()
    }

    /// A stream object: `dict_extra` is spliced into its dictionary alongside
    /// the `/Length` this computes.
    fn stream(&mut self, dict_extra: &[u8], data: &[u8]) -> usize {
        let mut body = Vec::with_capacity(data.len() + 64);
        body.extend_from_slice(b"<< /Length ");
        body.extend_from_slice(data.len().to_string().as_bytes());
        if !dict_extra.is_empty() {
            body.push(b' ');
            body.extend_from_slice(dict_extra);
        }
        body.extend_from_slice(b" >>\nstream\n");
        body.extend_from_slice(data);
        body.extend_from_slice(b"\nendstream");
        self.set_now(body)
    }

    fn finish(self, catalog: usize, info: Option<usize>) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        // The binary comment on line two is what tells a transfer agent this is
        // not text. Without it a file that survives a byte-for-byte copy can
        // still be mangled by one that "helpfully" converts line endings.
        out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets = Vec::with_capacity(self.bodies.len());
        for (index, body) in self.bodies.iter().enumerate() {
            offsets.push(out.len());
            let id = index + 1;
            out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
            // A reserved-but-unset object is a bug here rather than in the
            // file, so it is written as null instead of leaving a hole the
            // cross-reference table would point into.
            match body {
                Some(bytes) => out.extend_from_slice(bytes),
                None => out.extend_from_slice(b"null"),
            }
            out.extend_from_slice(b"\nendobj\n");
        }
        let startxref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", self.bodies.len() + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for offset in &offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        let mut trailer = format!(
            "trailer\n<< /Size {} /Root {catalog} 0 R",
            self.bodies.len() + 1
        );
        if let Some(info) = info {
            let _ = write!(trailer, " /Info {info} 0 R");
        }
        trailer.push_str(" >>\n");
        out.extend_from_slice(trailer.as_bytes());
        out.extend_from_slice(format!("startxref\n{startxref}\n%%EOF\n").as_bytes());
        out
    }
}

/// A PDF literal string with the four characters that would end it escaped.
fn pdf_string(text: &str) -> String {
    let mut out = String::from("(");
    for ch in text.chars() {
        match ch {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            // Outside Latin-1 there is no room in a literal string; the title
            // is metadata, so a replacement is better than a broken file.
            c if (c as u32) < 0x100 => out.push(c),
            _ => out.push('?'),
        }
    }
    out.push(')');
    out
}

/// A number a PDF parser will accept: fixed point, never an exponent, never
/// `NaN` or `inf` — any of which would make the file unopenable.
fn num(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_owned();
    }
    let mut s = format!("{value:.3}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" { "0".to_owned() } else { s }
}

// ---------------------------------------------------------------------------
// Fonts
// ---------------------------------------------------------------------------

/// Write every face as a Type0 font, returning the object id of each.
///
/// **Type0 / Identity-H, not a simple font.** A simple font addresses at most
/// 256 glyphs through an encoding, which cannot carry a sheet that mixes Greek
/// and Cyrillic — and a spreadsheet is exactly the document that does. Identity
/// addresses glyphs by id directly, and the `ToUnicode` map is what gives the
/// text back to a reader, a search box, or `pdftotext`. Without it the page
/// looks right and copies as mojibake, which is the failure mode this format
/// is famous for.
fn write_faces(objects: &mut Objects, faces: &Faces) -> Vec<usize> {
    let mut ids = Vec::with_capacity(faces.faces.len());
    for (index, face) in faces.faces.iter().enumerate() {
        let Ok(font) = FontRef::new(face.bytes) else {
            // A face that will not parse cannot be embedded. It also cannot
            // have produced a glyph — `page_content` parses before it records
            // one — so nothing on the page refers to this entry; a placeholder
            // keeps the resource numbering aligned.
            ids.push(
                objects.set_now(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec()),
            );
            continue;
        };
        let metrics = font.metrics(Size::unscaled(), LocationRef::default());
        let upem = f64::from(metrics.units_per_em.max(1));
        let to_thousandths = |v: f32| (f64::from(v) * 1000.0 / upem).round();
        let glyph_metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());

        let base = format!("OCF{index}");
        let file_id = objects.stream(
            format!("/Length1 {}", face.bytes.len()).as_bytes(),
            face.bytes,
        );

        let bounds = metrics.bounds.unwrap_or(skrifa::metrics::BoundingBox {
            x_min: 0.0,
            y_min: -200.0,
            x_max: 1000.0,
            y_max: 900.0,
        });
        // Symbolic, because the glyphs are addressed by id and not through a
        // standard encoding; plus the two flags a viewer uses to substitute
        // sensibly if the embedded file is ever stripped.
        let mut flags = 4u32;
        if metrics.is_monospace {
            flags |= 1;
        }
        if metrics.italic_angle != 0.0 {
            flags |= 64;
        }
        let descriptor = objects.set_now(
            format!(
                "<< /Type /FontDescriptor /FontName /{base} /Flags {flags} \
                 /FontBBox [{} {} {} {}] /ItalicAngle {} /Ascent {} /Descent {} \
                 /CapHeight {} /StemV 80 /FontFile2 {file_id} 0 R >>",
                num(to_thousandths(bounds.x_min)),
                num(to_thousandths(bounds.y_min)),
                num(to_thousandths(bounds.x_max)),
                num(to_thousandths(bounds.y_max)),
                num(f64::from(metrics.italic_angle)),
                num(to_thousandths(metrics.ascent)),
                num(to_thousandths(metrics.descent)),
                num(to_thousandths(metrics.cap_height.unwrap_or(metrics.ascent))),
            )
            .into_bytes(),
        );

        // Only the glyphs that were drawn: the widths of 3000 unused ones would
        // be a larger array than the page it describes.
        let mut widths = String::new();
        for &gid in face.used.keys() {
            let advance = glyph_metrics
                .advance_width(skrifa::GlyphId::new(gid))
                .unwrap_or(0.0);
            let _ = write!(widths, "{gid} [{}] ", num(to_thousandths(advance)));
        }

        let cid_font = objects.set_now(
            format!(
                "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{base} \
                 /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
                 /FontDescriptor {descriptor} 0 R /CIDToGIDMap /Identity /W [{}] >>",
                widths.trim_end()
            )
            .into_bytes(),
        );
        let to_unicode = objects.stream(b"", to_unicode_cmap(&face.used).as_bytes());
        ids.push(
            objects.set_now(
                format!(
                    "<< /Type /Font /Subtype /Type0 /BaseFont /{base} /Encoding /Identity-H \
                 /DescendantFonts [{cid_font} 0 R] /ToUnicode {to_unicode} 0 R >>"
                )
                .into_bytes(),
            ),
        );
    }
    ids
}

/// The `ToUnicode` CMap: what each glyph id means, so the text can be read back
/// out of the page.
fn to_unicode_cmap(used: &BTreeMap<u32, char>) -> String {
    let mut out = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    // `bfchar` blocks are capped at 100 entries by the specification, so a
    // sheet with more than a hundred distinct glyphs needs several.
    let entries: Vec<(u32, char)> = used.iter().map(|(&g, &c)| (g, c)).collect();
    for chunk in entries.chunks(100) {
        let _ = writeln!(out, "{} beginbfchar", chunk.len());
        for (gid, ch) in chunk {
            let mut utf16 = String::new();
            let mut buffer = [0u16; 2];
            for unit in ch.encode_utf16(&mut buffer) {
                let _ = write!(utf16, "{unit:04X}");
            }
            let _ = writeln!(out, "<{gid:04X}> <{utf16}>");
        }
        out.push_str("endbfchar\n");
    }
    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out
}

// ---------------------------------------------------------------------------
// Content
// ---------------------------------------------------------------------------

/// The content stream for one page.
fn page_content(
    page: &PdfPage<'_>,
    geometry: &GridGeometry,
    faces: &mut Faces,
    report: &mut ImageReport,
) -> String {
    let mut out = String::new();
    let scale = if page.scale.is_finite() && page.scale > 0.0 {
        page.scale
    } else {
        1.0
    };
    // One matrix for the whole page: twips in, points out, y flipped, scaled.
    let unit = scale / TWIPS_PER_POINT;
    let _ = writeln!(
        out,
        "q {} 0 0 {} {} {} cm",
        num(unit),
        num(-unit),
        num(page.margin_left as f64 / TWIPS_PER_POINT),
        num((page.height - page.margin_top) as f64 / TWIPS_PER_POINT),
    );

    for band in &page.bands {
        let source_x = geometry.columns.offset(band.cols.0);
        let source_y = geometry.rows.offset(band.rows.0);
        let width = geometry
            .columns
            .offset(band.cols.1.saturating_add(1))
            .saturating_sub(source_x);
        let height = geometry
            .rows
            .offset(band.rows.1.saturating_add(1))
            .saturating_sub(source_y);
        if width <= 0 || height <= 0 {
            continue;
        }
        // Translate so the band's own top-left lands on `origin`, then clip in
        // sheet coordinates — which is why the clip is written after the `cm`
        // and everything below can keep using the item rectangles unchanged.
        let _ = writeln!(
            out,
            "q 1 0 0 1 {} {} cm",
            num((band.origin.0 - source_x) as f64),
            num((band.origin.1 - source_y) as f64),
        );
        let _ = writeln!(
            out,
            "{} {} {} {} re W n",
            num(source_x as f64),
            num(source_y as f64),
            num(width as f64),
            num(height as f64),
        );
        if band.gridlines {
            write_gridlines(&mut out, geometry, band);
        }
        for item in &band.display_list.items {
            write_item(&mut out, item, faces, report);
        }
        out.push_str("Q\n");
    }

    out.push_str("Q\n");
    out
}

/// The grid, under the items, at the band's own row and column boundaries.
fn write_gridlines(out: &mut String, geometry: &GridGeometry, band: &PdfBand<'_>) {
    let x0 = geometry.columns.offset(band.cols.0);
    let x1 = geometry.columns.offset(band.cols.1.saturating_add(1));
    let y0 = geometry.rows.offset(band.rows.0);
    let y1 = geometry.rows.offset(band.rows.1.saturating_add(1));
    let _ = writeln!(
        out,
        "q {g} {g} {g} RG {w} w",
        g = num(GRID_GREY),
        w = num(HAIRLINE_TWIPS)
    );
    for col in band.cols.0..=band.cols.1.saturating_add(1) {
        let x = geometry.columns.offset(col);
        let _ = writeln!(
            out,
            "{} {} m {} {} l",
            num(x as f64),
            num(y0 as f64),
            num(x as f64),
            num(y1 as f64)
        );
    }
    for row in band.rows.0..=band.rows.1.saturating_add(1) {
        let y = geometry.rows.offset(row);
        let _ = writeln!(
            out,
            "{} {} m {} {} l",
            num(x0 as f64),
            num(y as f64),
            num(x1 as f64),
            num(y as f64)
        );
    }
    out.push_str("S\nQ\n");
}

/// `RRGGBB` as PDF's three fractions, or `None` when it is not hex.
fn rgb(hex: &str) -> Option<(f64, f64, f64)> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok().map(f64::from);
    Some((byte(0)? / 255.0, byte(2)? / 255.0, byte(4)? / 255.0))
}

fn fill_rect(out: &mut String, rect: &LayoutRect, color: (f64, f64, f64)) {
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }
    let _ = writeln!(
        out,
        "{} {} {} rg {} {} {} {} re f",
        num(color.0),
        num(color.1),
        num(color.2),
        num(rect.x as f64),
        num(rect.y as f64),
        num(rect.w as f64),
        num(rect.h as f64)
    );
}

fn write_item(out: &mut String, item: &PaintItem, faces: &mut Faces, report: &mut ImageReport) {
    match item {
        PaintItem::CellBackground { rect, fill } => {
            if let Some(color) = fill.as_deref().and_then(rgb) {
                fill_rect(out, rect, color);
            }
        }
        PaintItem::GridLine { rect } => {
            // A zero-height or zero-width rectangle. Stroked rather than
            // filled, so a one-twip line is still visible on paper.
            let _ = writeln!(
                out,
                "q {g} {g} {g} RG {w} w {} {} m {} {} l S Q",
                num(rect.x as f64),
                num(rect.y as f64),
                num((rect.x + rect.w) as f64),
                num((rect.y + rect.h) as f64),
                g = num(GRID_GREY),
                w = num(HAIRLINE_TWIPS)
            );
        }
        PaintItem::MergedRegion { rect, fill } => {
            // Fill first, outline second — the order the display list's own
            // documentation fixes, because outlining first paints it away.
            fill_rect(
                out,
                rect,
                fill.as_deref().and_then(rgb).unwrap_or((1.0, 1.0, 1.0)),
            );
            let _ = writeln!(
                out,
                "q {g} {g} {g} RG {w} w {} {} {} {} re S Q",
                num(rect.x as f64),
                num(rect.y as f64),
                num(rect.w as f64),
                num(rect.h as f64),
                g = num(GRID_GREY),
                w = num(HAIRLINE_TWIPS)
            );
        }
        PaintItem::DataBar {
            rect,
            fraction,
            color,
        } => {
            let fraction = fraction.clamp(0.0, 1.0);
            if fraction <= 0.0 || rect.w <= 0 || rect.h <= 0 {
                return;
            }
            let inset = TEXT_PAD_TWIPS.min(rect.w as f64 / 4.0);
            let width = ((rect.w as f64 - inset * 2.0).max(0.0)) * fraction;
            let color = rgb(color).unwrap_or((0.24, 0.51, 0.78));
            let _ = writeln!(
                out,
                "q /GSa gs {} {} {} rg {} {} {} {} re f Q",
                num(color.0),
                num(color.1),
                num(color.2),
                num(rect.x as f64 + inset),
                num(rect.y as f64 + inset),
                num(width),
                num(rect.h as f64 - inset * 2.0)
            );
        }
        PaintItem::Polyline {
            points,
            width,
            color,
        } => {
            if points.len() < 2 {
                return;
            }
            let Some(color) = rgb(color) else { return };
            let _ = writeln!(
                out,
                "q {} {} {} RG {} w",
                num(color.0),
                num(color.1),
                num(color.2),
                num((*width).max(1) as f64)
            );
            write_path(out, points);
            out.push_str("S\nQ\n");
        }
        PaintItem::Polygon { points, fill } => {
            if points.len() < 3 {
                return;
            }
            let Some(color) = rgb(fill) else { return };
            let _ = writeln!(
                out,
                "q {} {} {} rg",
                num(color.0),
                num(color.1),
                num(color.2)
            );
            write_path(out, points);
            // Non-zero winding, which is `f` — the rule the display list names.
            out.push_str("h f\nQ\n");
        }
        PaintItem::Wedge {
            center,
            radius,
            inner_radius,
            from,
            sweep,
            fill,
        } => {
            write_wedge(out, *center, *radius, *inner_radius, *from, *sweep, fill);
        }
        PaintItem::Text {
            rect,
            content,
            align,
            color,
            bold,
            italic,
            font_name,
            font_pt,
        } => {
            write_text(
                out,
                rect,
                content,
                *align,
                color.as_deref(),
                *bold,
                *italic,
                font_name.as_deref(),
                font_pt.unwrap_or(DEFAULT_FONT_PT),
                faces,
            );
        }
        PaintItem::Image { rect: _, part } => {
            // Not drawn, and said so — through the same deduplicating recorder
            // the raster path uses, so a picture on four pages is one thing
            // wrong said once. A picture-shaped hole in a printout is otherwise
            // indistinguishable from a sheet that never had one.
            report.missed(part, UndrawnReason::UnsupportedByBackend);
        }
        PaintItem::CellBorder {
            rect,
            left,
            right,
            top,
            bottom,
        } => {
            let edge = |out: &mut String, line: &Option<BorderLine>, x0, y0, x1, y1| {
                let Some(line) = line else { return };
                let color = line
                    .color
                    .as_deref()
                    .and_then(rgb)
                    .unwrap_or((0.0, 0.0, 0.0));
                let _ = writeln!(
                    out,
                    "q {} {} {} RG {} w {} {} m {} {} l S Q",
                    num(color.0),
                    num(color.1),
                    num(color.2),
                    num(f64::from(line.width.max(1)) * TWIPS_PER_PX as f64),
                    num(x0),
                    num(y0),
                    num(x1),
                    num(y1)
                );
            };
            let (x, y, w, h) = (rect.x as f64, rect.y as f64, rect.w as f64, rect.h as f64);
            edge(out, top, x, y, x + w, y);
            edge(out, bottom, x, y + h, x + w, y + h);
            edge(out, left, x, y, x, y + h);
            edge(out, right, x + w, y, x + w, y + h);
        }
    }
}

fn write_path(out: &mut String, points: &[LayoutPoint]) {
    for (i, point) in points.iter().enumerate() {
        let _ = writeln!(
            out,
            "{} {} {}",
            num(point.x as f64),
            num(point.y as f64),
            if i == 0 { "m" } else { "l" }
        );
    }
}

/// A point on a circle, in the display list's convention: degrees **clockwise
/// from twelve o'clock**, in a y-down space. The same arithmetic the raster
/// backend's `arc_point` does, so the two draw one pie.
fn arc_point(cx: f64, cy: f64, r: f64, angle_deg: f64) -> (f64, f64) {
    let t = angle_deg.to_radians();
    (cx + r * t.sin(), cy - r * t.cos())
}

fn arc_tangent(angle_deg: f64) -> (f64, f64) {
    let t = angle_deg.to_radians();
    (t.cos(), t.sin())
}

/// Append cubic segments approximating an arc, at most 90° each.
fn arc_to(out: &mut String, cx: f64, cy: f64, r: f64, from: f64, sweep: f64) {
    if sweep == 0.0 || r <= 0.0 {
        return;
    }
    let segments = (sweep.abs() / 90.0).ceil().max(1.0);
    let step = sweep / segments;
    let k = (4.0 / 3.0) * (step.to_radians() / 4.0).tan() * r;
    let mut a0 = from;
    for _ in 0..(segments as u32) {
        let a1 = a0 + step;
        let (x0, y0) = arc_point(cx, cy, r, a0);
        let (x1, y1) = arc_point(cx, cy, r, a1);
        let (tx0, ty0) = arc_tangent(a0);
        let (tx1, ty1) = arc_tangent(a1);
        let _ = writeln!(
            out,
            "{} {} {} {} {} {} c",
            num(x0 + k * tx0),
            num(y0 + k * ty0),
            num(x1 - k * tx1),
            num(y1 - k * ty1),
            num(x1),
            num(y1)
        );
        a0 = a1;
    }
}

#[allow(clippy::too_many_arguments)]
fn write_wedge(
    out: &mut String,
    center: LayoutPoint,
    radius: i64,
    inner_radius: i64,
    from: f64,
    sweep: f64,
    fill: &str,
) {
    if radius <= 0 || !from.is_finite() || !sweep.is_finite() || sweep == 0.0 {
        return;
    }
    let Some(color) = rgb(fill) else { return };
    let inner = inner_radius.clamp(0, radius) as f64;
    let r = radius as f64;
    let sweep = sweep.clamp(-360.0, 360.0);
    let to = from + sweep;
    let (cx, cy) = (center.x as f64, center.y as f64);

    let _ = writeln!(
        out,
        "q {} {} {} rg",
        num(color.0),
        num(color.1),
        num(color.2)
    );
    let (sx, sy) = if inner > 0.0 {
        arc_point(cx, cy, inner, from)
    } else {
        (cx, cy)
    };
    let _ = writeln!(out, "{} {} m", num(sx), num(sy));
    let (ox, oy) = arc_point(cx, cy, r, from);
    let _ = writeln!(out, "{} {} l", num(ox), num(oy));
    arc_to(out, cx, cy, r, from, sweep);
    if inner > 0.0 {
        let (ix, iy) = arc_point(cx, cy, inner, to);
        let _ = writeln!(out, "{} {} l", num(ix), num(iy));
        arc_to(out, cx, cy, inner, to, -sweep);
    }
    out.push_str("h f\nQ\n");
}

/// One run of characters drawn from one face.
struct Run {
    face: usize,
    /// Glyph id and the character it came from, in order.
    glyphs: Vec<(u32, char)>,
    /// The run's total advance, in twips at the requested size.
    advance: f64,
}

#[allow(clippy::too_many_arguments)]
fn write_text(
    out: &mut String,
    rect: &LayoutRect,
    content: &str,
    align: Align,
    color: Option<&str>,
    bold: bool,
    italic: bool,
    font_name: Option<&str>,
    font_pt: f32,
    faces: &mut Faces,
) {
    if content.is_empty() || font_pt <= 0.0 {
        return;
    }
    let size = f64::from(font_pt) * TWIPS_PER_POINT;
    let primary = fonts::face_bytes_for(font_name, bold, italic);
    let Ok(font) = FontRef::new(primary) else {
        return;
    };
    let metrics = font.metrics(Size::unscaled(), LocationRef::default());
    let upem = f64::from(metrics.units_per_em.max(1));

    // Split into runs by the face that covers each character, exactly as the
    // raster backend's per-`char` loop falls back — so the two put the same
    // glyphs in the same places.
    let mut runs: Vec<Run> = Vec::new();
    for ch in content.chars() {
        let (bytes, gid, advance) = match resolve(primary, &font, ch, bold, italic) {
            Some(resolved) => resolved,
            // Nothing covers it. The raster backend advances by zero and draws
            // nothing; matching that keeps the two runs the same width.
            None => continue,
        };
        let index = faces.index(bytes);
        let advance = advance / upem_of(bytes, upem) * size;
        match runs.last_mut() {
            Some(run) if run.face == index => {
                run.glyphs.push((gid, ch));
                run.advance += advance;
            }
            _ => runs.push(Run {
                face: index,
                glyphs: vec![(gid, ch)],
                advance,
            }),
        }
    }
    if runs.is_empty() {
        return;
    }

    let total: f64 = runs.iter().map(|r| r.advance).sum();
    let (x, y, w, h) = (rect.x as f64, rect.y as f64, rect.w as f64, rect.h as f64);
    let mut pen = match align {
        Align::Left => x + TEXT_PAD_TWIPS,
        Align::Right => x + w - TEXT_PAD_TWIPS - total,
        Align::Center => x + (w - total) / 2.0,
    };
    // The same vertical centring the raster backend does: the ascent/descent
    // band centred in the cell, the baseline an ascent below its top.
    let ascent = f64::from(metrics.ascent) / upem * size;
    let descent = f64::from(metrics.descent) / upem * size;
    let baseline = y + ((h - (ascent - descent)) / 2.0).max(0.0) + ascent;

    let (r, g, b) = color.and_then(rgb).unwrap_or((0.0, 0.0, 0.0));
    for run in &runs {
        let mut hex = String::with_capacity(run.glyphs.len() * 4);
        for (gid, ch) in &run.glyphs {
            // Identity-H addresses glyphs with two bytes. No TrueType face has
            // more than 65535, so this only ever skips a corrupt one.
            if *gid > 0xFFFF {
                continue;
            }
            let _ = write!(hex, "{gid:04X}");
            faces.note(run.face, *gid, *ch);
        }
        if !hex.is_empty() {
            let _ = writeln!(
                out,
                "BT /F{} {} Tf {} {} {} rg 1 0 0 -1 {} {} Tm <{hex}> Tj ET",
                run.face,
                num(size),
                num(r),
                num(g),
                num(b),
                num(pen),
                num(baseline)
            );
        }
        pen += run.advance;
    }
}

/// The units-per-em of a face, falling back to the primary's when it will not
/// parse (it already did once, to produce a glyph id).
fn upem_of(bytes: &[u8], fallback: f64) -> f64 {
    FontRef::new(bytes)
        .ok()
        .map(|f| {
            f64::from(
                f.metrics(Size::unscaled(), LocationRef::default())
                    .units_per_em
                    .max(1),
            )
        })
        .unwrap_or(fallback)
}

/// The face, glyph id and unscaled advance for one character: the requested
/// face when it covers it, otherwise the coverage fallback.
fn resolve<'a>(
    primary_bytes: &'static [u8],
    primary: &FontRef<'a>,
    ch: char,
    bold: bool,
    italic: bool,
) -> Option<(&'static [u8], u32, f64)> {
    let unscaled = Size::unscaled();
    let loc = LocationRef::default();
    if let Some(gid) = primary.charmap().map(ch) {
        let advance = primary.glyph_metrics(unscaled, loc).advance_width(gid);
        return Some((
            primary_bytes,
            gid.to_u32(),
            f64::from(advance.unwrap_or(0.0)),
        ));
    }
    let bytes = fonts::coverage_face_bytes(ch, bold, italic)?;
    let font = FontRef::new(bytes).ok()?;
    let gid = font.charmap().map(ch)?;
    let advance = font.glyph_metrics(unscaled, loc).advance_width(gid);
    Some((bytes, gid.to_u32(), f64::from(advance.unwrap_or(0.0))))
}
