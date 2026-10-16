# 101 — Japanese, Chinese and Korean fonts

Status: **Implemented** (`XL-FEAT-140`). Owner decision 2026-10-17: the CJK
faces ship as separate files loaded on demand, never inside the WebAssembly
binary; size is not a hurdle, and the desktop build and the site ship them as
static assets.

## What Excel does

A cell names one font. Excel's East Asian fonts — Yu Gothic, MS Gothic/Mincho,
Meiryo, SimSun, Microsoft YaHei, DengXian, PMingLiU, Microsoft JhengHei, Malgun
Gothic, Batang, Gulim and the rest — draw everything in the cell, Latin
included. A Latin font (Calibri) holding Japanese is drawn through Windows
**font linking**: the missing characters come from an East Asian face chosen by
the system locale. The same Han code point has four regional forms (JIS, GB,
Big5, KS), so "a CJK font" is four faces. Bold in MS Mincho, SimSun, Batang and
PMingLiU — which have no bold — is drawn by emboldening the regular outlines.

## The faces

Noto Sans / Serif CJK as published in google/fonts (OFL-1.1): the Japanese,
Simplified Chinese, Traditional Chinese and Korean variable fonts, instanced to
static **Sans 400 and 700** and **Serif 400** (no serif bold, as Excel's own
serif faces have none; it is synthesised). Twelve variants.

Each variant is cut into **unicode-range slices** (WOFF2) using Google Fonts'
own frequency-ordered partition per region, plus tail slices for every
character that partition drops (Google Fonts serves only ~16 k of SC's 30 k
characters; a rare surname still has to render). `tools/fonts/build-cjk-slices.py`
produces them reproducibly: sources pinned to a google/fonts commit and SHA-256,
fontTools pinned, timestamps not recalculated. WOFF2 is written with the **null
`glyf` transform**, so the engine's decoder is brotli plus table reassembly
(`casual-calc-text::woff2`) rather than the glyf reconstruction algorithm.

| Region | Variants | Slices per variant | Bytes (all variants) |
|---|---|---|---|
| JP | sans 400/700, serif 400 | 122 | 10.5 MB |
| SC | sans 400/700, serif 400 | 145 | 21.1 MB |
| TC | sans 400/700, serif 400 | 129 | 14.5 MB |
| KR | sans 400/700, serif 400 | 138 | 11.3 MB |
| **All** | 12 | 1,602 files | **57.3 MB** |

`webapp/fonts/cjk/` holds the slices, `manifest.json` (per region the slices'
ranges; per variant the family, weight and a SHA-256 prefix per file, used as
a cache-busting query), one `{region}.ranges` per region for hosts without a
JSON parser, and the two OFL texts.

## Name mapping

`casual_calc_text::cjk::east_asian` maps Excel's East Asian names — English and
native, with full-width letters folded (`ＭＳ Ｐゴシック` = `MS PGothic`) and weight
suffixes stripped (`Yu Gothic Medium`) — to a region and a style. The saved file
keeps the name it had; the mapping only decides what draws the cell.

## Which face draws a character

`faces::TextFaces`, built once per run of text, decides per character for the
canvas stack, the measurement, the raster backend and the PDF writer alike:

1. an East Asian font draws everything its slice covers, Latin included;
2. otherwise the cell's own (substituted) Latin face, if it has the glyph;
3. CJK text: the run's region — the font's, else kana → JP, hangul → KR,
   bopomofo → TC, else the host's default (Excel's system locale; the editor
   passes the browser's language, Japanese otherwise) — then the other regions;
4. anything else: the supplied and bundled faces, as before.

On screen the same order is the CSS stack `css_stack_for(name, region)`:
`"Carlito", "Calibri", "OpenCalc CJK JP Sans", "… SC Sans", "… TC Sans", "… KR Sans", sans-serif`.

## Measuring before the face arrives

Every ideograph, kana and full-width form in the shipped faces is exactly one
em; half-width forms half an em; hangul 0.92 em (sans) / 0.966 em (serif). These
are asserted against every slice. The engine measures an unloaded CJK character
at that width, so a measurement taken early is already right for the CJK part.
The editor's autofit measures with the browser; when a run's face is still
loading it fits once with the stand-in and **fits again when the face arrives**,
unless the column was resized in between. A face arriving also clears the width
cache and redraws.

## Export

Before a PDF export the editor asks `session_export_pdf_font_needs(sheet)` —
the writer itself runs and reports which variant it wanted for which
characters — fetches the matching slices from the same files the browser uses,
and registers them (`register_font_slice`). A native host does the same with
`cjk::SliceDirectory`. The PDF embeds a **subset** of each face: only the glyphs
drawn, at their original ids (`render::subset`), as `CIDFontType2` with a
`ToUnicode` map, so the text extracts back as the same characters. Bold serif is
drawn fill-and-stroke (`2 Tr`). Characters still undrawn are named per script,
with a sample, in the export's compatibility report; the report is capped at
65,536 distinct characters with an exact total.

## Measured

- The fixture (`fixtures/generated/cjk-east-asian.xlsx`, twelve short cells
  across every region and style) fetches **34 slices** to draw on screen and
  **32** for its PDF; the PDF embeds 114 KB of font subsets (2.28 MB if the
  slices were embedded whole).
- The WebAssembly binary grows by the brotli decoder only; no font bytes.

## Limits

- Vertical text, CJK line breaking and East Asian width in alignment are
  `XL-FEAT-141`. The slices carry no `vert`/`vrt2` features yet.
- MS Gothic's Latin is half-width fixed pitch; Noto's is proportional, so Latin
  in an MS Gothic cell is narrower than in Excel.
- The PNG renderer draws whatever slices the host registered; the editor's
  thumbnail does not fetch them.
