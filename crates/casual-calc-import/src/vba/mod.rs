//! A **read-only** decoder for the VBA project a macro-enabled workbook
//! carries (`xl/vbaProject.bin`), so a person can *see* the macros their file
//! holds (`MACRO-05`, docs/99 §4).
//!
//! # What this is not
//!
//! VBA is never executed here, and this decoder is never on the save path: the
//! project travels as a retained part, byte for byte, whatever this module
//! makes of it. Decoding is something a viewer asks for, after the file is
//! open; a project this cannot read is still a project the next save writes
//! back unchanged.
//!
//! # The format
//!
//! `vbaProject.bin` is a Compound File (`[MS-CFB]`, module `cfb`) holding a VBA
//! storage (`[MS-OVBA]`). Its `VBA/dir` stream, compressed with the
//! `[MS-OVBA]` §2.4.1 algorithm (module `ovba`), lists the project's code page and
//! its modules, each with the stream that holds it and the offset in that
//! stream (`MODULEOFFSET`) where the compressed **source text** begins — before
//! it is compiled p-code, which this does not read. The `PROJECT` stream, plain
//! text, says which modules are documents, classes or designers.
//!
//! # Hostile input (contract)
//!
//! Every failure is a typed [`VbaError`]; nothing panics. Sector chains are
//! cycle-checked, stream sizes are checked against the file before anything is
//! allocated, decompression is capped at [`MAX_DECOMPRESSED`] bytes **for the
//! whole project**, and at most [`MAX_MODULES`] modules are read. One module
//! that will not decode does not hide the others: its error is kept on the
//! module ([`VbaModule::source`]).

mod cfb;
mod ovba;
#[cfg(test)]
mod tests;

use std::fmt;

/// The most decompressed bytes one project may produce, across the `dir`
/// stream and every module together. 16 MiB is far beyond any real project's
/// source and far below what would hurt a browser tab.
pub const MAX_DECOMPRESSED: usize = 16 * 1024 * 1024;

/// The most modules one project may declare. Excel's own limits are lower in
/// practice; a `dir` stream claiming more is not a project anyone wrote.
pub const MAX_MODULES: usize = 1024;

/// Why a VBA project, or one module of it, could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum VbaError {
    /// The bytes are not a Compound File (no `D0 CF 11 E0` signature).
    NotCompoundFile,
    /// A sector size other than the two `[MS-CFB]` defines (512 and 4096).
    UnsupportedSectorSize(u16),
    /// The file ends before the named structure does.
    Truncated(&'static str),
    /// A sector id that is not in the file or not in its allocation table.
    BadSector(u32),
    /// A sector chain that loops back on itself.
    ChainCycle,
    /// A stream claims more bytes than the file could hold.
    OversizedStream(u64),
    /// The directory is malformed (a link out of range, a cycle, no root).
    BadDirectory(&'static str),
    /// There is no `VBA/dir` stream, so no project to read.
    NoProject,
    /// A compressed container is malformed.
    BadCompressedContainer(&'static str),
    /// Decompression would pass [`MAX_DECOMPRESSED`].
    DecompressedTooLarge,
    /// The `dir` stream is malformed.
    BadDirStream(&'static str),
    /// The project declares more than [`MAX_MODULES`] modules.
    TooManyModules(usize),
    /// A module's stream is missing from the `VBA` storage.
    ModuleStreamMissing(String),
    /// A module's source offset lies past the end of its stream — the shape a
    /// project has when its source was removed and only p-code is left.
    SourceOffsetOutOfRange {
        /// The offset the `dir` stream gives.
        offset: u32,
        /// The stream's length.
        stream_len: usize,
    },
}

impl fmt::Display for VbaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotCompoundFile => f.write_str("not a VBA project (no compound-file signature)"),
            Self::UnsupportedSectorSize(s) => {
                write!(f, "unsupported compound-file sector shift {s}")
            }
            Self::Truncated(what) => write!(f, "the VBA project is truncated ({what})"),
            Self::BadSector(id) => write!(
                f,
                "the VBA project names sector {id}, which it does not have"
            ),
            Self::ChainCycle => f.write_str("the VBA project's sector chain loops"),
            Self::OversizedStream(n) => {
                write!(f, "a VBA stream claims {n} bytes, more than the file holds")
            }
            Self::BadDirectory(why) => write!(f, "the VBA project's directory is malformed: {why}"),
            Self::NoProject => f.write_str("the file has no VBA/dir stream"),
            Self::BadCompressedContainer(why) => write!(f, "malformed compressed VBA data: {why}"),
            Self::DecompressedTooLarge => write!(
                f,
                "the VBA project decompresses past the {} MiB limit",
                MAX_DECOMPRESSED / (1024 * 1024)
            ),
            Self::BadDirStream(why) => write!(f, "the VBA dir stream is malformed: {why}"),
            Self::TooManyModules(n) => {
                write!(
                    f,
                    "the VBA project declares {n} modules (limit {MAX_MODULES})"
                )
            }
            Self::ModuleStreamMissing(name) => write!(f, "module stream {name:?} is missing"),
            Self::SourceOffsetOutOfRange { offset, stream_len } => write!(
                f,
                "the module's source offset {offset} is past its stream ({stream_len} bytes) — \
                 the source may have been removed, leaving only compiled code"
            ),
        }
    }
}

impl std::error::Error for VbaError {}

/// What kind of module this is, as the VBA editor groups them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VbaModuleKind {
    /// A standard (procedural) module — `Module1`.
    Standard,
    /// A class module.
    Class,
    /// A document module: `ThisWorkbook` or a sheet.
    Document,
    /// A designer module: a UserForm's code.
    Designer,
}

impl VbaModuleKind {
    /// A stable lower-case name, for JSON.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Class => "class",
            Self::Document => "document",
            Self::Designer => "designer",
        }
    }
}

/// One module of a decoded project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VbaModule {
    /// The module's name (`Module1`, `ThisWorkbook`).
    pub name: String,
    /// What kind of module it is.
    pub kind: VbaModuleKind,
    /// Its source text, decoded with the project's code page — or why it could
    /// not be read. VBA ends lines with CRLF, and the text keeps them.
    pub source: Result<String, VbaError>,
}

/// A decoded VBA project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VbaProject {
    /// The project's name (`VBAProject`).
    pub name: String,
    /// The Windows code page its text is written in (`PROJECTCODEPAGE`).
    pub code_page: u16,
    /// Whether this decoder knows that code page. When it does not, ASCII is
    /// still read correctly and every other byte becomes U+FFFD.
    pub code_page_supported: bool,
    /// The modules, in the order the project lists them.
    pub modules: Vec<VbaModule>,
}

/// Decode the VBA project in `bytes` (the content of `vbaProject.bin`).
///
/// # Errors
///
/// A [`VbaError`] when the container or its `dir` stream cannot be read. A
/// single module that cannot be read is not an error here; it carries its own.
pub fn decode_vba_project(bytes: &[u8]) -> Result<VbaProject, VbaError> {
    let cf = cfb::CompoundFile::open(bytes)?;
    let mut budget = MAX_DECOMPRESSED;
    let dir_raw = cf.stream("VBA/dir")?.ok_or(VbaError::NoProject)?;
    let dir = ovba::decompress(&dir_raw, &mut budget)?;
    let info = parse_dir(&dir)?;
    let codec = Codec::of(info.code_page);

    // The PROJECT stream says which non-standard modules are documents,
    // classes or designers. It is optional to this reader: without it a
    // non-standard module is called a class.
    let mut doc_kinds: Vec<(String, VbaModuleKind)> = Vec::new();
    if let Some(project) = cf.stream("PROJECT")? {
        let text = codec.decode(&project);
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let kind = match key.trim() {
                "Document" => VbaModuleKind::Document,
                "Class" => VbaModuleKind::Class,
                "BaseClass" => VbaModuleKind::Designer,
                "Module" => VbaModuleKind::Standard,
                _ => continue,
            };
            // `Document=ThisWorkbook/&H00000000`: the name is before the slash.
            let name = value.split('/').next().unwrap_or("").trim().to_owned();
            doc_kinds.push((name, kind));
        }
    }

    let streams = cf.streams_in("VBA")?;
    let mut modules = Vec::with_capacity(info.modules.len());
    for raw in info.modules {
        let name = raw
            .name_unicode
            .clone()
            .unwrap_or_else(|| codec.decode(&raw.name));
        let kind = if raw.procedural {
            VbaModuleKind::Standard
        } else {
            doc_kinds
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(&name))
                .map_or(VbaModuleKind::Class, |&(_, k)| k)
        };
        let stream_name = raw
            .stream_unicode
            .clone()
            .unwrap_or_else(|| codec.decode(&raw.stream));
        let source = read_module(&cf, &streams, &stream_name, raw.offset, &mut budget)
            .map(|bytes| codec.decode(&bytes));
        // The budget is the whole project's: once it is spent, each module
        // after it says so rather than the project failing as a whole.
        modules.push(VbaModule { name, kind, source });
    }
    Ok(VbaProject {
        name: codec.decode(&info.name),
        code_page: info.code_page,
        code_page_supported: codec.supported(),
        modules,
    })
}

fn read_module(
    cf: &cfb::CompoundFile<'_>,
    streams: &[(String, usize)],
    stream_name: &str,
    offset: u32,
    budget: &mut usize,
) -> Result<Vec<u8>, VbaError> {
    let want = stream_name.to_uppercase();
    let entry = streams
        .iter()
        .find(|(n, _)| *n == want)
        .map(|&(_, i)| i)
        .ok_or_else(|| VbaError::ModuleStreamMissing(stream_name.to_owned()))?;
    let bytes = cf
        .read(entry)?
        .ok_or_else(|| VbaError::ModuleStreamMissing(stream_name.to_owned()))?;
    let source = bytes
        .get(offset as usize..)
        .filter(|s| !s.is_empty())
        .ok_or(VbaError::SourceOffsetOutOfRange {
            offset,
            stream_len: bytes.len(),
        })?;
    ovba::decompress(source, budget)
}

/// A module as the `dir` stream describes it, before decoding.
#[derive(Debug, Default)]
struct RawModule {
    name: Vec<u8>,
    name_unicode: Option<String>,
    stream: Vec<u8>,
    stream_unicode: Option<String>,
    offset: u32,
    procedural: bool,
}

#[derive(Debug, Default)]
struct DirInfo {
    code_page: u16,
    name: Vec<u8>,
    modules: Vec<RawModule>,
}

fn utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

/// Read the decompressed `dir` stream (`[MS-OVBA]` §2.3.4.2).
///
/// Every record is `id: u16, size: u32, data[size]` — with one exception the
/// spec keeps for compatibility: `PROJECTVERSION` (`0x0009`) says size 4 and
/// carries 6 bytes. Records this does not use are skipped by their size, which
/// is what lets one loop read the project, reference and module sections alike.
fn parse_dir(d: &[u8]) -> Result<DirInfo, VbaError> {
    let mut info = DirInfo::default();
    let mut current: Option<RawModule> = None;
    let mut pos = 0usize;
    let mut declared: Option<usize> = None;
    while pos < d.len() {
        let head = d
            .get(pos..pos + 6)
            .ok_or(VbaError::BadDirStream("truncated record header"))?;
        let id = u16::from_le_bytes([head[0], head[1]]);
        let size = u32::from_le_bytes([head[2], head[3], head[4], head[5]]) as usize;
        pos += 6;
        let len = if id == 0x0009 { 6 } else { size };
        let data = pos
            .checked_add(len)
            .and_then(|end| d.get(pos..end))
            .ok_or(VbaError::BadDirStream("a record runs past the stream"))?;
        pos += len;
        match id {
            0x0003 => {
                let b = data
                    .get(..2)
                    .ok_or(VbaError::BadDirStream("short PROJECTCODEPAGE"))?;
                info.code_page = u16::from_le_bytes([b[0], b[1]]);
            }
            0x0004 => info.name = data.to_vec(),
            0x000F => {
                let b = data
                    .get(..2)
                    .ok_or(VbaError::BadDirStream("short PROJECTMODULES"))?;
                let n = usize::from(u16::from_le_bytes([b[0], b[1]]));
                if n > MAX_MODULES {
                    return Err(VbaError::TooManyModules(n));
                }
                declared = Some(n);
            }
            0x0019 => {
                if current.is_some() {
                    return Err(VbaError::BadDirStream("a module without its terminator"));
                }
                if info.modules.len() >= MAX_MODULES {
                    return Err(VbaError::TooManyModules(info.modules.len() + 1));
                }
                current = Some(RawModule {
                    name: data.to_vec(),
                    ..RawModule::default()
                });
            }
            0x0047 | 0x001A | 0x0032 | 0x0031 | 0x0021 | 0x0022 => {
                let m = current
                    .as_mut()
                    .ok_or(VbaError::BadDirStream("a module record outside a module"))?;
                match id {
                    0x0047 => m.name_unicode = Some(utf16(data)),
                    0x001A => m.stream = data.to_vec(),
                    0x0032 => m.stream_unicode = Some(utf16(data)),
                    0x0031 => {
                        let b = data
                            .get(..4)
                            .ok_or(VbaError::BadDirStream("short MODULEOFFSET"))?;
                        m.offset = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                    }
                    0x0021 => m.procedural = true,
                    _ => m.procedural = false,
                }
            }
            0x002B => {
                let m = current.take().ok_or(VbaError::BadDirStream(
                    "a module terminator outside a module",
                ))?;
                info.modules.push(m);
            }
            0x0010 => break,
            _ => {}
        }
    }
    if current.is_some() {
        return Err(VbaError::BadDirStream("the last module has no terminator"));
    }
    if declared.is_none() && info.modules.is_empty() {
        return Err(VbaError::BadDirStream("no PROJECTMODULES record"));
    }
    Ok(info)
}

/// The code pages this decoder reads. Others decode ASCII and replace the rest.
#[derive(Debug, Clone, Copy)]
enum Codec {
    Windows1252,
    Latin1,
    Utf8,
    /// ASCII; `known` when the code page *is* US-ASCII rather than one this
    /// decoder does not read.
    Ascii {
        known: bool,
    },
}

/// Windows-1252's 0x80–0x9F, where it differs from Latin-1. `0` marks the five
/// bytes it leaves undefined.
const CP1252_HIGH: [u16; 32] = [
    0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
    0x0152, 0, 0x017D, 0, 0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC,
    0x2122, 0x0161, 0x203A, 0x0153, 0, 0x017E, 0x0178,
];

impl Codec {
    fn of(code_page: u16) -> Self {
        match code_page {
            1252 => Self::Windows1252,
            28591 => Self::Latin1,
            65001 => Self::Utf8,
            20127 => Self::Ascii { known: true },
            _ => Self::Ascii { known: false },
        }
    }

    fn supported(self) -> bool {
        !matches!(self, Self::Ascii { known: false })
    }

    fn decode(self, bytes: &[u8]) -> String {
        match self {
            Self::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
            Self::Latin1 => bytes.iter().map(|&b| char::from(b)).collect(),
            Self::Windows1252 => bytes
                .iter()
                .map(|&b| match b {
                    0x80..=0x9F => char::from_u32(u32::from(CP1252_HIGH[usize::from(b - 0x80)]))
                        .filter(|&c| c != '\0')
                        .unwrap_or(char::REPLACEMENT_CHARACTER),
                    _ => char::from(b),
                })
                .collect(),
            Self::Ascii { .. } => bytes
                .iter()
                .map(|&b| {
                    if b.is_ascii() {
                        char::from(b)
                    } else {
                        char::REPLACEMENT_CHARACTER
                    }
                })
                .collect(),
        }
    }
}
