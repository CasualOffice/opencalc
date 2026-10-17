//! Japanese, Chinese and Korean text: which face draws it (`XL-FEAT-140`).
//!
//! # What Excel does, and what this copies
//!
//! A cell names one font. When that font is Latin (Calibri) and the text is
//! not, Windows **font linking** draws the missing characters from an East
//! Asian face, chosen by the system's language; when the font *is* an East
//! Asian one (Yu Gothic, MS Mincho, SimSun, PMingLiU, Malgun Gothic, …) it
//! draws everything, Latin included. The same Han character is a different
//! shape in Japan, mainland China, Taiwan and Korea, so "a CJK font" is not
//! one face but four.
//!
//! Here, in that order:
//!
//! 1. [`east_asian`] maps Excel's East Asian font names — English and native,
//!    full-width letters and all — to a **region** and a **style** (sans or
//!    serif). A cell styled `ＭＳ 明朝` is Japanese serif. The saved file keeps the
//!    name it had; this only decides what draws it.
//! 2. [`region_for`] picks the region for a Latin-font cell: the text's own
//!    kana or hangul when it has them, else the host's default (Excel's
//!    "system locale"), set with [`set_default_region`].
//! 3. The faces are **slices** — a few hundred characters each, WOFF2,
//!    `webapp/fonts/cjk/` — that a host fetches when a document needs them and
//!    hands to [`register_slice`]. Nothing is compiled in: the four regions
//!    are tens of megabytes, and a sheet in Japanese touches a dozen slices.
//!
//! # Measuring before the face arrives
//!
//! Every ideograph, kana and hangul syllable in these faces (and in Excel's
//! own East Asian fonts) is exactly **one em** wide, and the half-width forms
//! half of one. So [`nominal_advance_em`] gives the width a character will
//! have before its slice has loaded, and a column measured early does not need
//! measuring again for the CJK part of its text.

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use std::sync::atomic::{AtomicU8, Ordering};

use skrifa::{FontRef, MetadataProvider};

/// One of the four CJK regional glyph standards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Region {
    /// Japanese (JIS).
    Jp,
    /// Simplified Chinese (mainland China, GB).
    Sc,
    /// Traditional Chinese (Taiwan, Big5).
    Tc,
    /// Korean (KS).
    Kr,
}

impl Region {
    /// Every region, in the fallback order a character no earlier region has
    /// is looked for in.
    pub const ALL: [Region; 4] = [Region::Jp, Region::Sc, Region::Tc, Region::Kr];

    /// The short code used in file names and host APIs: `jp`, `sc`, `tc`, `kr`.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Jp => "jp",
            Self::Sc => "sc",
            Self::Tc => "tc",
            Self::Kr => "kr",
        }
    }

    /// The region a code names. Also accepts BCP 47 language tags (`ja`,
    /// `zh-CN`, `zh-Hant`, `ko-KR`), so a host can pass its UI language.
    #[must_use]
    pub fn parse(code: &str) -> Option<Self> {
        let c = code.trim().to_ascii_lowercase();
        Some(match c.as_str() {
            "jp" | "ja" | "jpn" => Self::Jp,
            "sc" | "zh" | "zh-cn" | "zh-sg" | "zh-hans" | "zh-hans-cn" => Self::Sc,
            "tc" | "zh-tw" | "zh-hk" | "zh-mo" | "zh-hant" | "zh-hant-tw" | "zh-hant-hk" => {
                Self::Tc
            }
            "kr" | "ko" | "ko-kr" | "kor" => Self::Kr,
            _ if c.starts_with("ja") => Self::Jp,
            _ if c.starts_with("ko") => Self::Kr,
            _ if c.starts_with("zh-hant") || c.starts_with("zh-tw") || c.starts_with("zh-hk") => {
                Self::Tc
            }
            _ if c.starts_with("zh") => Self::Sc,
            _ => return None,
        })
    }

    fn index(self) -> u8 {
        match self {
            Self::Jp => 0,
            Self::Sc => 1,
            Self::Tc => 2,
            Self::Kr => 3,
        }
    }

    fn from_index(i: u8) -> Self {
        match i {
            1 => Self::Sc,
            2 => Self::Tc,
            3 => Self::Kr,
            _ => Self::Jp,
        }
    }
}

/// Gothic (sans) or Mincho/Song/Myeongjo (serif).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Style {
    /// Gothic / Hei / Dotum.
    Sans,
    /// Mincho / Song / Ming / Batang.
    Serif,
}

impl Style {
    fn code(self) -> &'static str {
        match self {
            Self::Sans => "sans",
            Self::Serif => "serif",
        }
    }
}

/// One shipped face: a region, a style and a weight. Sans ships Regular and
/// Bold; serif ships Regular only, and its bold is drawn emboldened — which is
/// what Windows does for MS Mincho, SimSun, Batang and PMingLiU, none of which
/// has a bold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Variant {
    /// Which regional glyph standard.
    pub region: Region,
    /// Sans or serif.
    pub style: Style,
    /// 400 or 700.
    pub weight: u16,
}

impl Variant {
    /// The variant that draws `style` text in `region`, bold or not, taking
    /// the shipped weights into account.
    #[must_use]
    pub fn new(region: Region, style: Style, bold: bool) -> Self {
        let weight = if bold && style == Style::Sans {
            700
        } else {
            400
        };
        Self {
            region,
            style,
            weight,
        }
    }

    /// The key the slices are filed under: `jp-sans-400`.
    #[must_use]
    pub fn key(self) -> String {
        format!(
            "{}-{}-{}",
            self.region.code(),
            self.style.code(),
            self.weight
        )
    }

    /// The variant a key names.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        let mut parts = key.split('-');
        let region = Region::parse(parts.next()?)?;
        let style = match parts.next()? {
            "sans" => Style::Sans,
            "serif" => Style::Serif,
            _ => return None,
        };
        let weight = match parts.next()? {
            "400" => 400,
            "700" if style == Style::Sans => 700,
            _ => return None,
        };
        parts.next().is_none().then_some(Self {
            region,
            style,
            weight,
        })
    }

    /// The CSS family the editor registers this variant's slices under.
    #[must_use]
    pub fn css_family(self) -> String {
        css_family(self.region, self.style)
    }

    /// The same style and weight in another region.
    #[must_use]
    pub fn in_region(self, region: Region) -> Self {
        Self { region, ..self }
    }

    /// This variant first, then the other regions in [`Region::ALL`] order:
    /// where a character the first region lacks is looked for, on screen and
    /// in the exporters alike.
    #[must_use]
    pub fn chain(self) -> [Variant; 4] {
        let mut out = [self; 4];
        let mut i = 1;
        for r in Region::ALL {
            if r != self.region {
                out[i] = self.in_region(r);
                i += 1;
            }
        }
        out
    }
}

/// The CSS family name for a region and style: `OpenCalc CJK JP Sans`.
#[must_use]
pub fn css_family(region: Region, style: Style) -> String {
    let r = match region {
        Region::Jp => "JP",
        Region::Sc => "SC",
        Region::Tc => "TC",
        Region::Kr => "KR",
    };
    let s = match style {
        Style::Sans => "Sans",
        Style::Serif => "Serif",
    };
    format!("OpenCalc CJK {r} {s}")
}

/// Fold a font name for lookup: full-width ASCII to ASCII, the ideographic
/// space to a space, ASCII lowercased, runs of spaces collapsed. Excel writes
/// `ＭＳ Ｐゴシック` with full-width letters; users type `MS PGothic`; both are the
/// same font.
fn fold(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut space = false;
    for ch in name.trim().chars() {
        let ch = match u32::from(ch) {
            0xFF01..=0xFF5E => char::from_u32(u32::from(ch) - 0xFEE0).unwrap_or(ch),
            0x3000 => ' ',
            _ => ch,
        };
        if ch == ' ' {
            if !space && !out.is_empty() {
                out.push(' ');
            }
            space = true;
            continue;
        }
        space = false;
        out.push(ch.to_ascii_lowercase());
    }
    out.trim_end().to_owned()
}

/// Excel's (and the platforms') East Asian font names, to the region and style
/// that stands in for them.
///
/// English and native names both, because Excel writes whichever the UI
/// language used: a file saved on Japanese Windows says `游ゴシック`, the same
/// font on English Windows says `Yu Gothic`.
#[must_use]
pub fn east_asian(name: &str) -> Option<(Region, Style)> {
    use Region::{Jp, Kr, Sc, Tc};
    use Style::{Sans, Serif};
    let key = fold(name);
    // Weight suffixes name the same family: "Yu Gothic Medium", "游ゴシック Light".
    let base = [
        "light", "medium", "regular", "bold", "semibold", "demibold", "ui",
    ]
    .iter()
    .fold(key.as_str(), |k, suffix| {
        k.strip_suffix(suffix).map_or(k, str::trim_end)
    })
    .to_owned();
    let hit = |k: &str| -> Option<(Region, Style)> {
        Some(match k {
            // Japanese gothic.
            "ms gothic"
            | "ms pgothic"
            | "ms ui gothic"
            | "msゴシック"
            | "ms ゴシック"
            | "ms pゴシック"
            | "meiryo"
            | "メイリオ"
            | "yu gothic"
            | "游ゴシック"
            | "游ゴシック体"
            | "yugothic"
            | "biz udgothic"
            | "biz udpgothic"
            | "biz udゴシック"
            | "biz udpゴシック"
            | "hiragino sans"
            | "hiragino kaku gothic pro"
            | "hiragino kaku gothic pron"
            | "ヒラギノ角ゴ pro"
            | "ヒラギノ角ゴ pron"
            | "ヒラギノ角ゴシック"
            | "osaka"
            | "noto sans jp"
            | "noto sans cjk jp"
            | "source han sans jp"
            | "source han sans"
            | "ipagothic"
            | "ipaexgothic"
            | "ipaゴシック"
            | "ipaexゴシック"
            | "takaogothic" => (Jp, Sans),
            // Japanese mincho.
            "ms mincho"
            | "ms pmincho"
            | "ms 明朝"
            | "ms p明朝"
            | "ms明朝"
            | "yu mincho"
            | "游明朝"
            | "游明朝体"
            | "yumincho"
            | "biz udmincho"
            | "biz udpmincho"
            | "biz ud明朝"
            | "biz udp明朝"
            | "hiragino mincho pro"
            | "hiragino mincho pron"
            | "ヒラギノ明朝 pro"
            | "ヒラギノ明朝 pron"
            | "noto serif jp"
            | "noto serif cjk jp"
            | "source han serif jp"
            | "source han serif"
            | "ipamincho"
            | "ipaexmincho"
            | "ipa明朝"
            | "ipaex明朝" => (Jp, Serif),
            // Simplified Chinese hei.
            "microsoft yahei"
            | "微软雅黑"
            | "dengxian"
            | "等线"
            | "simhei"
            | "黑体"
            | "pingfang sc"
            | "苹方-简"
            | "heiti sc"
            | "stheiti"
            | "华文黑体"
            | "noto sans sc"
            | "noto sans cjk sc"
            | "source han sans sc"
            | "source han sans cn"
            | "思源黑体"
            | "wenquanyi micro hei"
            | "文泉驿微米黑" => (Sc, Sans),
            // Simplified Chinese song (and kai/fangsong, which are closer to
            // song than to hei).
            "simsun"
            | "宋体"
            | "nsimsun"
            | "新宋体"
            | "simsun-extb"
            | "fangsong"
            | "仿宋"
            | "kaiti"
            | "楷体"
            | "stsong"
            | "华文宋体"
            | "stkaiti"
            | "华文楷体"
            | "stfangsong"
            | "华文仿宋"
            | "songti sc"
            | "宋体-简"
            | "noto serif sc"
            | "noto serif cjk sc"
            | "source han serif sc"
            | "source han serif cn"
            | "思源宋体" => (Sc, Serif),
            // Traditional Chinese hei.
            "microsoft jhenghei" | "微軟正黑體" | "pingfang tc" | "pingfang hk" | "蘋方-繁"
            | "heiti tc" | "黑體-繁" | "noto sans tc" | "noto sans hk" | "noto sans cjk tc"
            | "source han sans tc" | "source han sans tw" | "思源黑體" => (Tc, Sans),
            // Traditional Chinese ming.
            "pmingliu"
            | "新細明體"
            | "mingliu"
            | "細明體"
            | "mingliu_hkscs"
            | "細明體_hkscs"
            | "pmingliu-extb"
            | "mingliu-extb"
            | "dfkai-sb"
            | "標楷體"
            | "songti tc"
            | "宋體-繁"
            | "lisong pro"
            | "儷宋 pro"
            | "noto serif tc"
            | "noto serif cjk tc"
            | "source han serif tc"
            | "source han serif tw"
            | "思源宋體" => (Tc, Serif),
            // Korean gothic / dotum.
            "malgun gothic"
            | "맑은 고딕"
            | "gulim"
            | "굴림"
            | "gulimche"
            | "굴림체"
            | "dotum"
            | "돋움"
            | "dotumche"
            | "돋움체"
            | "apple sd gothic neo"
            | "애플 sd 산돌고딕 neo"
            | "nanum gothic"
            | "나눔고딕"
            | "nanumgothic"
            | "noto sans kr"
            | "noto sans cjk kr"
            | "source han sans kr"
            | "source han sans k"
            | "본고딕" => (Kr, Sans),
            // Korean batang / myeongjo.
            "batang"
            | "바탕"
            | "batangche"
            | "바탕체"
            | "gungsuh"
            | "궁서"
            | "gungsuhche"
            | "궁서체"
            | "applemyungjo"
            | "nanum myeongjo"
            | "나눔명조"
            | "nanummyeongjo"
            | "noto serif kr"
            | "noto serif cjk kr"
            | "source han serif kr"
            | "source han serif k"
            | "본명조" => (Kr, Serif),
            _ => return None,
        })
    };
    hit(&key).or_else(|| hit(&base))
}

/// Whether `ch` is East Asian text that the CJK faces, rather than a Latin
/// face, should draw when the cell's own font has no glyph for it.
#[must_use]
pub fn is_cjk(ch: char) -> bool {
    matches!(u32::from(ch),
        0x1100..=0x11FF   // Hangul Jamo
        | 0x2E80..=0x2FDF // CJK radicals, Kangxi radicals
        | 0x2FF0..=0x2FFF // ideographic description
        | 0x3000..=0x303F // CJK symbols and punctuation
        | 0x3040..=0x30FF // hiragana, katakana
        | 0x3100..=0x312F // bopomofo
        | 0x3130..=0x318F // hangul compatibility jamo
        | 0x3190..=0x31FF // kanbun, bopomofo ext, strokes, katakana ext
        | 0x3200..=0x33FF // enclosed CJK, CJK compatibility
        | 0x3400..=0x4DBF // extension A
        | 0x4E00..=0x9FFF // unified ideographs
        | 0xA960..=0xA97F // hangul jamo ext A
        | 0xAC00..=0xD7AF // hangul syllables
        | 0xD7B0..=0xD7FF // hangul jamo ext B
        | 0xF900..=0xFAFF // compatibility ideographs
        | 0xFE10..=0xFE1F // vertical forms
        | 0xFE30..=0xFE4F // CJK compatibility forms
        | 0xFF00..=0xFFEF // half-width and full-width forms
        | 0x1F200..=0x1F2FF // enclosed ideographic supplement
        | 0x20000..=0x3134F // extensions B–G
    )
}

/// The width a CJK character has in every shipped face of `style`, in ems,
/// when that is fixed: one em for ideographs, kana and full-width forms, half
/// for the half-width kana and symbols, and the shipped faces' hangul width —
/// 0.92 em in the sans, 0.966 in the serif — for hangul (Korean faces set
/// hangul narrower than Han). `None` for characters whose width depends on the
/// face (Latin, combining marks, conjoining jamo).
///
/// Checked against every slice by
/// `the_nominal_width_is_the_real_width_in_every_shipped_slice`.
#[must_use]
pub fn nominal_advance_em(ch: char, style: Style) -> Option<f32> {
    let hangul = match style {
        Style::Sans => 0.92,
        Style::Serif => 0.966,
    };
    match u32::from(ch) {
        0xFF61..=0xFF9D | 0xFFE8..=0xFFEE => Some(0.5),
        0x3131..=0x318E | 0xAC00..=0xD7A3 | 0xFFA0..=0xFFDC => Some(hangul),
        0x3041..=0x3096
        | 0x309B..=0x30FF
        | 0x3105..=0x312F
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xF900..=0xFAFF
        | 0xFF01..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x20000..=0x3134F => Some(1.0),
        _ => None,
    }
}

/// Whether `ch` decides a Latin-font cell's region on its own: kana is only
/// Japanese, hangul only Korean, bopomofo only Traditional Chinese.
fn script_region(ch: char) -> Option<Region> {
    match u32::from(ch) {
        0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F => Some(Region::Jp),
        0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF | 0xFFA0..=0xFFDC => {
            Some(Region::Kr)
        }
        0x3100..=0x312F | 0x31A0..=0x31BF => Some(Region::Tc),
        _ => None,
    }
}

static DEFAULT_REGION: AtomicU8 = AtomicU8::new(0);

/// The region a Latin-font cell's Han characters are drawn in when nothing in
/// the cell says otherwise — Excel's equivalent is the Windows system locale.
/// Japanese until a host says otherwise: on English Windows the font-link
/// chain for the UI faces lists the Japanese faces first.
#[must_use]
pub fn default_region() -> Region {
    Region::from_index(DEFAULT_REGION.load(Ordering::Relaxed))
}

/// Set [`default_region`]. A host policy, like the locale it stands for: the
/// editor passes the viewer's language, a server its deployment's.
pub fn set_default_region(region: Region) {
    DEFAULT_REGION.store(region.index(), Ordering::Relaxed);
}

/// The region `text` in a cell styled `family` is drawn in.
///
/// The font's own region when it is an East Asian font; else the first
/// script in the text that belongs to one region alone (kana, hangul,
/// bopomofo); else [`default_region`]. Bounded: the scan stops at the first
/// decisive character and never reads more than the first 4096.
#[must_use]
pub fn region_for(family: Option<&str>, text: &str) -> Region {
    if let Some((region, _)) = family.and_then(east_asian) {
        return region;
    }
    text.chars()
        .take(4096)
        .find_map(script_region)
        .unwrap_or_else(default_region)
}

// ---------------------------------------------------------------------------
// The slice registry
// ---------------------------------------------------------------------------

/// The largest decoded slice accepted. The shipped ones are under 200 KB.
pub const MAX_SLICE_BYTES: usize = 16 << 20;
/// The most decoded bytes kept across every slice: four regions of every
/// shipped variant fit with room to spare.
pub const MAX_TOTAL_SLICE_BYTES: usize = 512 << 20;
/// The most slices kept.
pub const MAX_SLICES: usize = 8192;

#[derive(Default)]
struct Registry {
    faces: Vec<&'static [u8]>,
    total: usize,
    /// (variant, character) → index into `faces`. First registration wins, so
    /// the slices' disjoint partition is never overridden by a later one.
    index: HashMap<(Variant, char), u32>,
    /// What has been registered, by variant and content fingerprint.
    seen: HashSet<(Variant, u64)>,
}

static REGISTRY: RwLock<Option<Registry>> = RwLock::new(None);

/// Why a slice was not registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SliceError {
    /// Not a variant key this build knows.
    UnknownVariant,
    /// The bytes are not a face (WOFF2 or sfnt) that can be read.
    Unreadable(String),
    /// Past [`MAX_SLICE_BYTES`], [`MAX_TOTAL_SLICE_BYTES`] or [`MAX_SLICES`].
    Limit,
}

impl std::fmt::Display for SliceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownVariant => f.write_str("unknown CJK font variant"),
            Self::Unreadable(why) => write!(f, "unreadable font slice: {why}"),
            Self::Limit => f.write_str("font slice limit reached"),
        }
    }
}

impl std::error::Error for SliceError {}

fn fingerprint(bytes: &[u8]) -> u64 {
    // FNV-1a: deterministic across runs and platforms, which `DefaultHasher`
    // does not promise.
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Register one slice of `variant` (WOFF2 or sfnt bytes), returning how many
/// characters it maps. Idempotent by content.
///
/// # Errors
///
/// See [`SliceError`].
pub fn register_slice(variant: Variant, bytes: &[u8]) -> Result<usize, SliceError> {
    let sfnt = if crate::woff2::is_woff2(bytes) {
        crate::woff2::to_sfnt(bytes, MAX_SLICE_BYTES)
            .map_err(|e| SliceError::Unreadable(e.to_string()))?
    } else {
        if bytes.len() > MAX_SLICE_BYTES {
            return Err(SliceError::Limit);
        }
        bytes.to_vec()
    };
    let font = FontRef::new(&sfnt).map_err(|e| SliceError::Unreadable(e.to_string()))?;
    let chars: Vec<char> = font
        .charmap()
        .mappings()
        .filter_map(|(cp, _)| char::from_u32(cp))
        .collect();
    let print = fingerprint(&sfnt);
    let mut guard = REGISTRY
        .write()
        .map_err(|_| SliceError::Unreadable("registry poisoned".into()))?;
    let reg = guard.get_or_insert_with(Registry::default);
    if reg.seen.contains(&(variant, print)) {
        return Ok(chars.len());
    }
    if reg.faces.len() >= MAX_SLICES || reg.total + sfnt.len() > MAX_TOTAL_SLICE_BYTES {
        return Err(SliceError::Limit);
    }
    reg.total += sfnt.len();
    let leaked: &'static [u8] = Box::leak(sfnt.into_boxed_slice());
    let at = u32::try_from(reg.faces.len()).map_err(|_| SliceError::Limit)?;
    reg.faces.push(leaked);
    reg.seen.insert((variant, print));
    for ch in &chars {
        reg.index.entry((variant, *ch)).or_insert(at);
    }
    Ok(chars.len())
}

/// The registered slice of `variant` that maps `ch`, if one has been
/// registered.
#[must_use]
pub fn loaded_face(variant: Variant, ch: char) -> Option<&'static [u8]> {
    let guard = REGISTRY.read().ok()?;
    let reg = guard.as_ref()?;
    let at = *reg.index.get(&(variant, ch))?;
    reg.faces.get(at as usize).copied()
}

/// Whether any slice of any variant maps `ch`. For the coverage report.
#[must_use]
pub fn any_loaded(ch: char) -> Option<&'static [u8]> {
    let region = default_region();
    for style in [Style::Sans, Style::Serif] {
        for weight in [false, true] {
            for v in Variant::new(region, style, weight).chain() {
                if let Some(face) = loaded_face(v, ch) {
                    return Some(face);
                }
            }
        }
    }
    None
}

/// How many slices are registered, and their decoded size in bytes.
#[must_use]
pub fn registered_slices() -> (usize, usize) {
    REGISTRY
        .read()
        .ok()
        .and_then(|g| g.as_ref().map(|r| (r.faces.len(), r.total)))
        .unwrap_or((0, 0))
}

// ---------------------------------------------------------------------------
// Finding the slice a character is in
// ---------------------------------------------------------------------------

/// Which slice holds which characters, per region — `{region}.ranges` in the
/// slice directory, one line per slice in CSS `unicode-range` form without the
/// `U+` (`4e00-4e05,4e08`).
#[derive(Debug, Clone, Default)]
pub struct SliceIndex {
    /// Per region: `(first, last, slice)`, sorted by `first`.
    regions: HashMap<Region, Vec<(u32, u32, u32)>>,
}

impl SliceIndex {
    /// Add one region's `.ranges` text. Lines that do not parse are skipped
    /// rather than fatal: a slice that cannot be found is reported as a
    /// missing glyph, which is the honest outcome.
    pub fn add_region(&mut self, region: Region, text: &str) {
        let mut spans = Vec::new();
        for (slice, line) in text.lines().enumerate() {
            let Ok(slice) = u32::try_from(slice) else {
                break;
            };
            for part in line.split(',') {
                let part = part.trim();
                let (a, b) = part.split_once('-').unwrap_or((part, part));
                if let (Ok(a), Ok(b)) = (u32::from_str_radix(a, 16), u32::from_str_radix(b, 16))
                    && a <= b
                {
                    spans.push((a, b, slice));
                }
            }
        }
        spans.sort_unstable();
        self.regions.insert(region, spans);
    }

    /// The slice of `region` that holds `ch`.
    #[must_use]
    pub fn slice_for(&self, region: Region, ch: char) -> Option<u32> {
        let spans = self.regions.get(&region)?;
        let cp = u32::from(ch);
        let at = spans.partition_point(|&(a, _, _)| a <= cp);
        let &(a, b, slice) = spans.get(at.checked_sub(1)?)?;
        (a..=b).contains(&cp).then_some(slice)
    }

    /// The slice to fetch for a character wanted in `variant`: the first
    /// region along [`Variant::chain`] that has it — the same walk the
    /// browser makes down the CSS font stack.
    #[must_use]
    pub fn locate(&self, variant: Variant, ch: char) -> Option<(Variant, u32)> {
        variant
            .chain()
            .into_iter()
            .find_map(|v| self.slice_for(v.region, ch).map(|i| (v, i)))
    }
}

/// The relative path of one slice file: `jp-sans-400/007.woff2`.
#[must_use]
pub fn slice_path(variant: Variant, slice: u32) -> String {
    format!("{}/{slice:03}.woff2", variant.key())
}

/// A directory of slices on disk — `webapp/fonts/cjk/` — for a host that
/// renders natively (a server exporting PDFs, the command line, tests).
///
/// The browser host does the same thing with `fetch` (`editor.core.js`); this
/// is that loop for a host with a file system.
#[derive(Debug, Clone)]
pub struct SliceDirectory {
    root: std::path::PathBuf,
    index: SliceIndex,
}

impl SliceDirectory {
    /// Read the `.ranges` files under `root`.
    ///
    /// # Errors
    ///
    /// When none of the four can be read — `root` is not a slice directory.
    pub fn open(root: impl Into<std::path::PathBuf>) -> std::io::Result<Self> {
        let root = root.into();
        let mut index = SliceIndex::default();
        let mut found = false;
        for region in Region::ALL {
            if let Ok(text) =
                std::fs::read_to_string(root.join(format!("{}.ranges", region.code())))
            {
                index.add_region(region, &text);
                found = true;
            }
        }
        if !found {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("no CJK .ranges files under {}", root.display()),
            ));
        }
        Ok(Self { root, index })
    }

    /// The index this directory was read into.
    #[must_use]
    pub fn index(&self) -> &SliceIndex {
        &self.index
    }

    /// Register the slices that hold `wanted`, returning how many were newly
    /// read. Each slice is read at most once per call; a file that is missing
    /// or will not register is skipped, and its characters stay reported as
    /// missing by whatever asked.
    pub fn load(&self, wanted: impl IntoIterator<Item = (Variant, char)>) -> usize {
        let mut files: std::collections::BTreeSet<(Variant, u32)> = Default::default();
        for (variant, ch) in wanted {
            if let Some(hit) = self.index.locate(variant, ch) {
                files.insert(hit);
            }
        }
        files
            .into_iter()
            .filter(|&(variant, slice)| {
                std::fs::read(self.root.join(slice_path(variant, slice)))
                    .ok()
                    .is_some_and(|bytes| register_slice(variant, &bytes).is_ok())
            })
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_and_full_width_names_fold_to_the_same_font() {
        assert_eq!(
            east_asian("ＭＳ Ｐゴシック"),
            Some((Region::Jp, Style::Sans))
        );
        assert_eq!(east_asian("MS PGothic"), Some((Region::Jp, Style::Sans)));
        assert_eq!(east_asian("ＭＳ 明朝"), Some((Region::Jp, Style::Serif)));
        assert_eq!(
            east_asian("Yu Gothic Medium"),
            Some((Region::Jp, Style::Sans))
        );
        assert_eq!(
            east_asian("游ゴシック Light"),
            Some((Region::Jp, Style::Sans))
        );
        assert_eq!(east_asian("等线"), Some((Region::Sc, Style::Sans)));
        assert_eq!(east_asian("宋体"), Some((Region::Sc, Style::Serif)));
        assert_eq!(east_asian("新細明體"), Some((Region::Tc, Style::Serif)));
        assert_eq!(east_asian("微軟正黑體"), Some((Region::Tc, Style::Sans)));
        assert_eq!(east_asian("맑은 고딕"), Some((Region::Kr, Style::Sans)));
        assert_eq!(east_asian("바탕"), Some((Region::Kr, Style::Serif)));
        assert_eq!(east_asian("Calibri"), None);
        assert_eq!(east_asian("Gothic"), None, "no substring guessing");
    }

    #[test]
    fn variant_keys_round_trip_and_serif_has_no_bold() {
        for key in ["jp-sans-400", "jp-sans-700", "kr-serif-400", "tc-sans-700"] {
            assert_eq!(Variant::from_key(key).unwrap().key(), key);
        }
        assert_eq!(Variant::from_key("jp-serif-700"), None);
        assert_eq!(Variant::new(Region::Sc, Style::Serif, true).weight, 400);
        assert_eq!(
            Variant::new(Region::Tc, Style::Sans, false)
                .chain()
                .map(|v| v.region),
            [Region::Tc, Region::Jp, Region::Sc, Region::Kr]
        );
    }

    #[test]
    fn a_latin_cell_takes_its_region_from_kana_or_hangul() {
        assert_eq!(region_for(Some("Calibri"), "東京と大阪"), Region::Jp);
        assert_eq!(region_for(Some("Calibri"), "서울 漢字"), Region::Kr);
        assert_eq!(
            region_for(Some("宋体"), "ひらがな"),
            Region::Sc,
            "the font wins"
        );
        assert_eq!(region_for(None, "中文"), default_region());
    }
}
