//! Tests for the VBA decoder, and the **fixture builder** they share: a
//! minimal but valid Compound File writer and `[MS-OVBA]` project writer, so a
//! real `vbaProject.bin` can be made without Excel.

use super::*;

// ---------------------------------------------------------------- builders --

/// Compress `data` into an `[MS-OVBA]` container. Not a good compressor — full
/// 4096-byte chunks are stored raw, and a final partial chunk as literals only
/// — but a *valid* one, which is all a fixture needs; the copy-token path is
/// covered by the spec's own vectors below.
pub(crate) fn compress(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x01];
    for chunk in data.chunks(4096) {
        if chunk.len() == 4096 {
            // Uncompressed chunk: size field 4095 (→ 4098 bytes), flag clear.
            out.extend_from_slice(&0x3FFFu16.to_le_bytes());
            out.extend_from_slice(chunk);
        } else {
            let mut body = Vec::new();
            for group in chunk.chunks(8) {
                body.push(0); // all literals
                body.extend_from_slice(group);
            }
            assert!(
                body.len() + 2 <= 4098,
                "fixture chunk too large for literal-only coding"
            );
            let header = 0x8000 | 0x3000 | (body.len() + 2 - 3) as u16;
            out.extend_from_slice(&header.to_le_bytes());
            out.extend_from_slice(&body);
        }
    }
    out
}

const END: u32 = 0xFFFF_FFFE;
const FREE: u32 = 0xFFFF_FFFF;

struct Node {
    name: String,
    kind: u8,
    children: Vec<usize>,
    data: Vec<u8>,
    start: u32,
    size: u64,
    right: u32,
    child: u32,
}

/// Write a version-3 Compound File holding `streams` (`"VBA/dir"`-style paths;
/// storages are made as needed). Streams under 4096 bytes go in the mini
/// stream, as `[MS-CFB]` requires, so both allocation paths are exercised.
pub(crate) fn build_cfb(streams: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut nodes = vec![Node {
        name: "Root Entry".into(),
        kind: 5,
        children: vec![],
        data: vec![],
        start: END,
        size: 0,
        right: FREE,
        child: FREE,
    }];
    for (path, data) in streams {
        let mut at = 0;
        let parts: Vec<&str> = path.split('/').collect();
        for (k, part) in parts.iter().enumerate() {
            let last = k + 1 == parts.len();
            let existing = nodes[at]
                .children
                .iter()
                .copied()
                .find(|&c| nodes[c].name == *part);
            at = match existing {
                Some(c) => c,
                None => {
                    nodes.push(Node {
                        name: (*part).into(),
                        kind: if last { 2 } else { 1 },
                        children: vec![],
                        data: if last { data.clone() } else { vec![] },
                        start: END,
                        size: if last { data.len() as u64 } else { 0 },
                        right: FREE,
                        child: FREE,
                    });
                    let i = nodes.len() - 1;
                    nodes[at].children.push(i);
                    i
                }
            };
        }
    }
    // Siblings as a right-leaning chain: a valid search tree, if not a balanced one.
    for i in 0..nodes.len() {
        let kids = nodes[i].children.clone();
        if let Some(&first) = kids.first() {
            nodes[i].child = first as u32;
        }
        for w in kids.windows(2) {
            nodes[w[0]].right = w[1] as u32;
        }
    }

    // Mini stream.
    let mut mini = Vec::new();
    let mut minifat: Vec<u32> = Vec::new();
    for n in nodes.iter_mut().filter(|n| n.kind == 2 && n.size < 4096) {
        if n.size == 0 {
            continue;
        }
        let first = (mini.len() / 64) as u32;
        let count = n.data.len().div_ceil(64);
        for k in 0..count {
            minifat.push(if k + 1 == count {
                END
            } else {
                first + k as u32 + 1
            });
        }
        n.start = first;
        mini.extend_from_slice(&n.data);
        mini.resize(mini.len().div_ceil(64) * 64, 0);
    }
    let mini_len = mini.len() as u64;

    let pad = |mut v: Vec<u8>| {
        v.resize(v.len().div_ceil(512).max(1) * 512, 0);
        v
    };
    // Blobs in sector order after the FAT: directory, MiniFAT, mini stream, big streams.
    let dir_sectors = (nodes.len() * 128).div_ceil(512);
    let minifat_bytes: Vec<u8> = minifat.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut blobs: Vec<Vec<u8>> = vec![vec![0; dir_sectors * 512]];
    let has_minifat = !minifat.is_empty();
    if has_minifat {
        blobs.push(pad(minifat_bytes));
        blobs.push(pad(mini));
    }
    let big: Vec<usize> = (0..nodes.len())
        .filter(|&i| nodes[i].kind == 2 && nodes[i].size >= 4096)
        .collect();
    for &i in &big {
        blobs.push(pad(nodes[i].data.clone()));
    }
    let data_sectors: usize = blobs.iter().map(|b| b.len() / 512).sum();
    let mut fat_sectors = 1;
    while fat_sectors * 128 < fat_sectors + data_sectors {
        fat_sectors += 1;
    }
    assert!(
        fat_sectors <= 109,
        "fixture too large for a header-only DIFAT"
    );

    let mut fat = vec![0xFFFF_FFFDu32; fat_sectors];
    let mut starts = Vec::new();
    for b in &blobs {
        let first = fat.len() as u32;
        starts.push(first);
        let n = b.len() / 512;
        for k in 0..n {
            fat.push(if k + 1 == n {
                END
            } else {
                first + k as u32 + 1
            });
        }
    }
    fat.resize(fat_sectors * 128, FREE);

    let dir_start = starts[0];
    if has_minifat {
        nodes[0].start = starts[2];
        nodes[0].size = mini_len;
    }
    for (k, &i) in big.iter().enumerate() {
        nodes[i].start = starts[(if has_minifat { 3 } else { 1 }) + k];
    }
    let mut dir = Vec::new();
    for n in &nodes {
        let mut e = [0u8; 128];
        let units: Vec<u16> = n.name.encode_utf16().collect();
        for (k, u) in units.iter().enumerate() {
            e[2 * k..2 * k + 2].copy_from_slice(&u.to_le_bytes());
        }
        e[0x40..0x42].copy_from_slice(&(((units.len() + 1) * 2) as u16).to_le_bytes());
        e[0x42] = n.kind;
        e[0x43] = 1;
        e[0x44..0x48].copy_from_slice(&FREE.to_le_bytes());
        e[0x48..0x4C].copy_from_slice(&n.right.to_le_bytes());
        e[0x4C..0x50].copy_from_slice(&n.child.to_le_bytes());
        e[0x74..0x78].copy_from_slice(&n.start.to_le_bytes());
        e[0x78..0x80].copy_from_slice(&n.size.to_le_bytes());
        dir.extend_from_slice(&e);
    }
    // Unused entries in the last directory sector.
    while dir.len() < dir_sectors * 512 {
        let mut e = [0u8; 128];
        e[0x44..0x50].copy_from_slice(&[0xFF; 12]);
        dir.extend_from_slice(&e);
    }
    blobs[0] = dir;

    let mut header = vec![0u8; 512];
    header[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    header[0x18..0x1A].copy_from_slice(&0x3Eu16.to_le_bytes());
    header[0x1A..0x1C].copy_from_slice(&3u16.to_le_bytes());
    header[0x1C..0x1E].copy_from_slice(&0xFFFEu16.to_le_bytes());
    header[0x1E..0x20].copy_from_slice(&9u16.to_le_bytes());
    header[0x20..0x22].copy_from_slice(&6u16.to_le_bytes());
    header[0x2C..0x30].copy_from_slice(&(fat_sectors as u32).to_le_bytes());
    header[0x30..0x34].copy_from_slice(&dir_start.to_le_bytes());
    header[0x38..0x3C].copy_from_slice(&4096u32.to_le_bytes());
    let (mf_start, mf_count) = if has_minifat {
        (starts[1], (blobs[1].len() / 512) as u32)
    } else {
        (END, 0)
    };
    header[0x3C..0x40].copy_from_slice(&mf_start.to_le_bytes());
    header[0x40..0x44].copy_from_slice(&mf_count.to_le_bytes());
    header[0x44..0x48].copy_from_slice(&END.to_le_bytes());
    for k in 0..109 {
        let v = if k < fat_sectors { k as u32 } else { FREE };
        header[0x4C + 4 * k..0x50 + 4 * k].copy_from_slice(&v.to_le_bytes());
    }
    let mut out = header;
    out.extend(fat.iter().flat_map(|v| v.to_le_bytes()));
    for b in blobs {
        out.extend_from_slice(&b);
    }
    out
}

/// A module for [`build_project`].
pub(crate) struct FixtureModule<'a> {
    pub name: &'a str,
    /// `Module`, `Class`, `Document` or `BaseClass`, as the PROJECT stream says.
    pub project_key: &'a str,
    pub source: Vec<u8>,
    /// Bytes of stand-in p-code before the source.
    pub pcode: usize,
}

fn rec(out: &mut Vec<u8>, id: u16, data: &[u8]) {
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
}

fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// The decompressed `dir` stream for `modules`, with the project-information
/// and reference records a real one carries — including `PROJECTVERSION`'s
/// size-says-4-carries-6 quirk.
pub(crate) fn dir_stream(code_page: u16, modules: &[FixtureModule<'_>]) -> Vec<u8> {
    let mut d = Vec::new();
    rec(&mut d, 0x0001, &1u32.to_le_bytes());
    rec(&mut d, 0x004A, &1u32.to_le_bytes());
    rec(&mut d, 0x0002, &0x409u32.to_le_bytes());
    rec(&mut d, 0x0014, &0x409u32.to_le_bytes());
    rec(&mut d, 0x0003, &code_page.to_le_bytes());
    rec(&mut d, 0x0004, b"VBAProject");
    rec(&mut d, 0x0005, b"");
    rec(&mut d, 0x0040, b"");
    rec(&mut d, 0x0006, b"");
    rec(&mut d, 0x003D, b"");
    rec(&mut d, 0x0007, &0u32.to_le_bytes());
    rec(&mut d, 0x0008, &0u32.to_le_bytes());
    // PROJECTVERSION: Reserved=4 in the size slot, then major u32 + minor u16.
    d.extend_from_slice(&0x0009u16.to_le_bytes());
    d.extend_from_slice(&4u32.to_le_bytes());
    d.extend_from_slice(&1_234_567u32.to_le_bytes());
    d.extend_from_slice(&42u16.to_le_bytes());
    rec(&mut d, 0x000C, b"");
    rec(&mut d, 0x003C, b"");
    // One registered reference, as every Excel project has (stdole).
    rec(&mut d, 0x0016, b"stdole");
    rec(&mut d, 0x003E, &utf16le("stdole"));
    let libid = b"*\\G{00020430-0000-0000-C000-000000000046}#2.0#0#C:\\Windows\\System32\\stdole2.tlb#OLE Automation";
    let mut reg = Vec::new();
    reg.extend_from_slice(&(libid.len() as u32).to_le_bytes());
    reg.extend_from_slice(libid);
    reg.extend_from_slice(&[0; 6]);
    rec(&mut d, 0x000D, &reg);
    rec(&mut d, 0x000F, &(modules.len() as u16).to_le_bytes());
    rec(&mut d, 0x0013, &0xFFFFu16.to_le_bytes());
    for m in modules {
        rec(&mut d, 0x0019, m.name.as_bytes());
        rec(&mut d, 0x0047, &utf16le(m.name));
        rec(&mut d, 0x001A, m.name.as_bytes());
        rec(&mut d, 0x0032, &utf16le(m.name));
        rec(&mut d, 0x001C, b"");
        rec(&mut d, 0x0048, b"");
        rec(&mut d, 0x0031, &(m.pcode as u32).to_le_bytes());
        rec(&mut d, 0x001E, &0u32.to_le_bytes());
        rec(&mut d, 0x002C, &0xFFFFu16.to_le_bytes());
        rec(
            &mut d,
            if m.project_key == "Module" {
                0x0021
            } else {
                0x0022
            },
            b"",
        );
        rec(&mut d, 0x002B, b"");
    }
    rec(&mut d, 0x0010, b"");
    d
}

/// A whole `vbaProject.bin`.
pub(crate) fn build_project(code_page: u16, modules: &[FixtureModule<'_>]) -> Vec<u8> {
    let mut project = String::from("ID=\"{00000000-0000-0000-0000-000000000000}\"\r\n");
    for m in modules {
        match m.project_key {
            "Document" => project.push_str(&format!("Document={}/&H00000000\r\n", m.name)),
            key => project.push_str(&format!("{key}={}\r\n", m.name)),
        }
    }
    project.push_str("Name=\"VBAProject\"\r\nHelpContextID=\"0\"\r\n");
    let mut streams: Vec<(String, Vec<u8>)> = vec![
        (
            "VBA/_VBA_PROJECT".into(),
            vec![0xCC, 0x61, 0xFF, 0xFF, 0x00, 0x00, 0x00],
        ),
        ("VBA/dir".into(), compress(&dir_stream(code_page, modules))),
        ("PROJECT".into(), project.into_bytes()),
        ("PROJECTwm".into(), vec![0, 0]),
    ];
    for m in modules {
        let mut s = vec![0xAB; m.pcode];
        s.extend_from_slice(&compress(&m.source));
        streams.push((format!("VBA/{}", m.name), s));
    }
    let refs: Vec<(&str, Vec<u8>)> = streams
        .iter()
        .map(|(p, d)| (p.as_str(), d.clone()))
        .collect();
    build_cfb(&refs)
}

pub(crate) const HELLO: &str =
    "Attribute VB_Name = \"Module1\"\r\nSub Hello()\r\n    MsgBox \"Hello, world\"\r\nEnd Sub\r\n";

/// The standard fixture: a document module, a standard module with
/// `Sub Hello()`, and a class.
pub(crate) fn hello_project() -> Vec<u8> {
    build_project(
        1252,
        &[
            FixtureModule {
                name: "ThisWorkbook",
                project_key: "Document",
                source: b"Attribute VB_Name = \"ThisWorkbook\"\r\nAttribute VB_PredeclaredId = True\r\n".to_vec(),
                pcode: 0x20,
            },
            FixtureModule {
                name: "Module1",
                project_key: "Module",
                source: HELLO.as_bytes().to_vec(),
                pcode: 0x113,
            },
            FixtureModule {
                name: "Class1",
                project_key: "Class",
                // 0xE9 é and 0x80 € in Windows-1252.
                source: b"Attribute VB_Name = \"Class1\"\r\n' caf\xE9 \x80 5\r\nPublic Total As Long\r\n".to_vec(),
                pcode: 0,
            },
        ],
    )
}

fn hex(s: &str) -> Vec<u8> {
    s.split_whitespace()
        .map(|b| u8::from_str_radix(b, 16).unwrap())
        .collect()
}

fn decompress_all(data: &[u8]) -> Result<Vec<u8>, VbaError> {
    let mut budget = MAX_DECOMPRESSED;
    ovba::decompress(data, &mut budget)
}

// --------------------------------------------- [MS-OVBA] §3.2 test vectors --

#[test]
fn spec_vector_no_compression() {
    let c = hex(
        "01 19 B0 00 61 62 63 64 65 66 67 68 00 69 6A 6B 6C 6D 6E 6F 70 00 71 72 73 74 75 76 2E",
    );
    assert_eq!(decompress_all(&c).unwrap(), b"abcdefghijklmnopqrstuv.");
}

#[test]
fn spec_vector_normal_compression() {
    let c = hex(
        "01 2F B0 00 23 61 61 61 62 63 64 65 82 66 00 70 61 67 68 69 6A 01 38 08 61 6B 6C 00 20 6D 6E 6F \
         70 06 71 02 70 04 00 72 73 74 75 76 10 77 78 79 7A 00 2C",
    );
    assert_eq!(
        String::from_utf8(decompress_all(&c).unwrap()).unwrap(),
        "#aaabcdefaaaaghijaaaaaklaaamnopqaaaaaaaaaaaarstuvwxyzaaa"
    );
}

#[test]
fn spec_vector_maximum_compression() {
    let c = hex("01 03 B0 02 61 45 00");
    assert_eq!(decompress_all(&c).unwrap(), vec![b'a'; 73]);
}

#[test]
fn the_fixture_compressor_round_trips_through_the_decompressor() {
    for len in [0, 1, 7, 8, 9, 3000, 4096, 4097, 9000] {
        let data: Vec<u8> = (0..len).map(|i| (i * 7 % 251) as u8).collect();
        assert_eq!(decompress_all(&compress(&data)).unwrap(), data, "len {len}");
    }
}

// ------------------------------------------------------------ decoding ------

#[test]
fn decodes_a_project_with_its_modules_kinds_and_source() {
    let p = decode_vba_project(&hello_project()).unwrap();
    assert_eq!(p.name, "VBAProject");
    assert_eq!(p.code_page, 1252);
    assert!(p.code_page_supported);
    let summary: Vec<(&str, VbaModuleKind)> = p
        .modules
        .iter()
        .map(|m| (m.name.as_str(), m.kind))
        .collect();
    assert_eq!(
        summary,
        [
            ("ThisWorkbook", VbaModuleKind::Document),
            ("Module1", VbaModuleKind::Standard),
            ("Class1", VbaModuleKind::Class),
        ]
    );
    assert_eq!(p.modules[1].source.as_deref().unwrap(), HELLO);
    // Windows-1252, not Latin-1: 0x80 is the euro sign.
    assert!(
        p.modules[2]
            .source
            .as_deref()
            .unwrap()
            .contains("' café € 5")
    );
}

#[test]
fn a_large_module_spans_chunks_and_the_regular_fat() {
    // Over 4096 bytes compressed, so the stream leaves the mini stream; over
    // 4096 decompressed, so the container has more than one chunk.
    let mut source = String::from("Attribute VB_Name = \"Big\"\r\n");
    let mut i = 0;
    while source.len() < 12_000 {
        source.push_str(&format!("Sub P{i}()\r\n    Debug.Print {i}\r\nEnd Sub\r\n"));
        i += 1;
    }
    // Two full chunks and a short third, which the literal-only fixture
    // compressor can code.
    source.truncate(2 * 4096 + 2000);
    let bin = build_project(
        1252,
        &[FixtureModule {
            name: "Big",
            project_key: "Module",
            source: source.clone().into_bytes(),
            pcode: 600,
        }],
    );
    let p = decode_vba_project(&bin).unwrap();
    assert_eq!(p.modules[0].source.as_deref().unwrap(), source);
}

#[test]
fn utf8_and_unsupported_code_pages() {
    let m = |src: &[u8]| FixtureModule {
        name: "M",
        project_key: "Module",
        source: src.to_vec(),
        pcode: 4,
    };
    let p = decode_vba_project(&build_project(65001, &[m("' naïve\r\n".as_bytes())])).unwrap();
    assert!(p.code_page_supported);
    assert_eq!(p.modules[0].source.as_deref().unwrap(), "' naïve\r\n");

    // Shift-JIS: not decoded. ASCII survives; the rest is replaced, and the
    // project says so rather than showing mojibake as if it were right.
    let p = decode_vba_project(&build_project(932, &[m(b"Sub A()\r\n' \x82\xA0\r\n")])).unwrap();
    assert!(!p.code_page_supported);
    assert_eq!(
        p.modules[0].source.as_deref().unwrap(),
        "Sub A()\r\n' \u{FFFD}\u{FFFD}\r\n"
    );
}

// --------------------------------------------------------- hostile input ----

#[test]
fn not_a_compound_file() {
    assert_eq!(decode_vba_project(b""), Err(VbaError::NotCompoundFile));
    assert_eq!(
        decode_vba_project(&[0u8; 1024]),
        Err(VbaError::NotCompoundFile)
    );
    // The bytes the save-formats browser fixture carries: not a real project.
    assert_eq!(
        decode_vba_project(b"not a vba project"),
        Err(VbaError::NotCompoundFile)
    );
}

#[test]
fn every_truncation_is_an_error_or_a_shorter_read_never_a_panic() {
    let bin = hello_project();
    for len in 0..bin.len() {
        let _ = decode_vba_project(&bin[..len]);
    }
    // Cut inside the header: typed, not a panic.
    assert_eq!(
        decode_vba_project(&bin[..100]),
        Err(VbaError::NotCompoundFile)
    );
    // Cut after the FAT: the directory is gone.
    assert!(matches!(
        decode_vba_project(&bin[..1024]),
        Err(VbaError::BadSector(_))
    ));
}

/// The FAT sector is sector 0, at byte 512; entry `i` is at `512 + 4*i`.
fn set_fat(bin: &mut [u8], i: usize, v: u32) {
    bin[512 + 4 * i..516 + 4 * i].copy_from_slice(&v.to_le_bytes());
}

#[test]
fn a_fat_cycle_is_a_typed_error_not_a_hang() {
    let mut bin = hello_project();
    // Sector 1 is the directory's first sector: point it at itself.
    set_fat(&mut bin, 1, 1);
    assert_eq!(decode_vba_project(&bin), Err(VbaError::ChainCycle));
}

#[test]
fn a_fat_link_out_of_range_is_a_typed_error() {
    let mut bin = hello_project();
    set_fat(&mut bin, 1, 0x00FF_FFFF);
    assert!(matches!(
        decode_vba_project(&bin),
        Err(VbaError::BadSector(_))
    ));
}

/// The byte offset of directory entry `i` (the directory starts at sector 1).
fn entry_at(i: usize) -> usize {
    1024 + 128 * i
}

fn entry_named(bin: &[u8], name: &str) -> usize {
    let want: Vec<u8> = utf16le(name);
    (0..64)
        .find(|&i| bin[entry_at(i)..].starts_with(&want) && bin[entry_at(i) + want.len()] == 0)
        .expect("entry present")
}

#[test]
fn an_oversized_stream_claim_is_refused_before_allocation() {
    let mut bin = hello_project();
    let i = entry_named(&bin, "dir");
    // Four gigabytes, in a file of a few kilobytes.
    bin[entry_at(i) + 0x78..entry_at(i) + 0x7C].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
    assert_eq!(
        decode_vba_project(&bin),
        Err(VbaError::OversizedStream(0xFFFF_FFF0))
    );
}

#[test]
fn a_directory_cycle_is_a_typed_error() {
    let mut bin = hello_project();
    let i = entry_named(&bin, "dir");
    // Its right sibling is itself.
    bin[entry_at(i) + 0x48..entry_at(i) + 0x4C].copy_from_slice(&(i as u32).to_le_bytes());
    assert!(matches!(
        decode_vba_project(&bin),
        Err(VbaError::BadDirectory(_))
    ));
}

#[test]
fn a_bogus_dir_stream_is_a_typed_error() {
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (b"garbage that is not compressed".to_vec(), "container"),
        (compress(b"\x19\x00\xFF\xFF\xFF\x7F"), "record past end"),
        (compress(&[0x19, 0, 1, 0, 0, 0, b'A']), "no terminator"),
        (
            compress(&[0x0F, 0, 2, 0, 0, 0, 0xFF, 0xFF]),
            "too many modules",
        ),
        (compress(&[]), "empty"),
    ];
    for (dir, what) in cases {
        let bin = build_cfb(&[("VBA/dir", dir)]);
        let err = decode_vba_project(&bin).expect_err(what);
        assert!(
            matches!(
                err,
                VbaError::BadCompressedContainer(_)
                    | VbaError::BadDirStream(_)
                    | VbaError::TooManyModules(_)
            ),
            "{what}: {err:?}"
        );
    }
}

#[test]
fn a_project_without_a_dir_stream_is_no_project() {
    let bin = build_cfb(&[("Other", vec![1, 2, 3])]);
    assert_eq!(decode_vba_project(&bin), Err(VbaError::NoProject));
}

#[test]
fn a_pcode_only_module_keeps_its_error_and_the_others_still_read() {
    let mut modules = vec![
        FixtureModule {
            name: "Good",
            project_key: "Module",
            source: HELLO.as_bytes().to_vec(),
            pcode: 8,
        },
        FixtureModule {
            name: "Stomped",
            project_key: "Module",
            source: vec![],
            pcode: 40,
        },
    ];
    // The Stomped stream holds only its 40 bytes of p-code; point its source
    // offset past them, the way a project whose source was removed looks.
    modules[1].pcode = 40;
    let mut dir = dir_stream(1252, &modules);
    // Patch the second MODULEOFFSET to 4000.
    let needle = [0x31u8, 0, 4, 0, 0, 0, 40, 0, 0, 0];
    let at = dir.windows(needle.len()).position(|w| w == needle).unwrap();
    dir[at + 6..at + 10].copy_from_slice(&4000u32.to_le_bytes());
    let mut good = vec![0xAB; 8];
    good.extend_from_slice(&compress(HELLO.as_bytes()));
    let bin = build_cfb(&[
        ("VBA/dir", compress(&dir)),
        ("VBA/Good", good),
        ("VBA/Stomped", vec![0xAB; 40]),
    ]);
    let p = decode_vba_project(&bin).unwrap();
    assert_eq!(p.modules[0].source.as_deref().unwrap(), HELLO);
    assert!(matches!(
        p.modules[1].source,
        Err(VbaError::SourceOffsetOutOfRange {
            offset: 4000,
            stream_len: 40
        })
    ));
}

#[test]
fn a_missing_module_stream_is_named() {
    let modules = [FixtureModule {
        name: "Ghost",
        project_key: "Module",
        source: vec![],
        pcode: 0,
    }];
    let bin = build_cfb(&[("VBA/dir", compress(&dir_stream(1252, &modules)))]);
    let p = decode_vba_project(&bin).unwrap();
    assert_eq!(
        p.modules[0].source,
        Err(VbaError::ModuleStreamMissing("Ghost".into()))
    );
}

/// One chunk that decompresses to 4096 bytes in six: a literal and one copy
/// token of length 4095.
fn bomb_chunk() -> [u8; 6] {
    [0x03, 0xB0, 0x02, b'a', 0xFC, 0x0F]
}

#[test]
fn decompression_is_capped_for_the_whole_project() {
    // One container just past the cap, in about 24 KB.
    let mut bomb = vec![0x01];
    for _ in 0..(MAX_DECOMPRESSED / 4096 + 1) {
        bomb.extend_from_slice(&bomb_chunk());
    }
    assert_eq!(
        decompress_all(&bomb).map(|v| v.len()),
        Err(VbaError::DecompressedTooLarge)
    );

    // And the cap is the project's, not each module's: two modules of three
    // quarters of it each cannot both be read.
    let mut half = vec![0x01];
    for _ in 0..(MAX_DECOMPRESSED / 4096 * 3 / 4) {
        half.extend_from_slice(&bomb_chunk());
    }
    let modules = [
        FixtureModule {
            name: "A",
            project_key: "Module",
            source: vec![],
            pcode: 0,
        },
        FixtureModule {
            name: "B",
            project_key: "Module",
            source: vec![],
            pcode: 0,
        },
    ];
    let bin = build_cfb(&[
        ("VBA/dir", compress(&dir_stream(1252, &modules))),
        ("VBA/A", half.clone()),
        ("VBA/B", half),
    ]);
    let p = decode_vba_project(&bin).unwrap();
    assert_eq!(
        p.modules[0].source.as_ref().map(String::len),
        Ok(MAX_DECOMPRESSED / 4 * 3)
    );
    assert_eq!(p.modules[1].source, Err(VbaError::DecompressedTooLarge));
}

#[test]
fn a_copy_token_before_its_chunk_is_an_error() {
    // Flag 0b01: the first token is a copy, with nothing yet to copy from.
    assert!(matches!(
        decompress_all(&[0x01, 0x02, 0xB0, 0x01, 0x00, 0x00]),
        Err(VbaError::BadCompressedContainer(_))
    ));
    // A literal, then a copy reaching 2 bytes back.
    assert!(matches!(
        decompress_all(&[0x01, 0x03, 0xB0, 0x02, b'a', 0x00, 0x10]),
        Err(VbaError::BadCompressedContainer(_))
    ));
}

#[test]
fn deterministic_mutations_never_panic() {
    let bin = hello_project();
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..3000 {
        let mut m = bin.clone();
        for _ in 0..(next() % 8 + 1) {
            let at = (next() % m.len() as u64) as usize;
            m[at] = next() as u8;
        }
        let _ = decode_vba_project(&m);
    }
}

/// A macro-enabled package carrying `vba`: one sheet with `A1=41`, and the
/// project hung off the workbook by a `vbaProject` relationship.
pub(crate) fn xlsm_with(vba: &[u8]) -> Vec<u8> {
    use std::io::Write;
    use zip::write::{SimpleFileOptions, ZipWriter};
    let parts: [(&str, &[u8]); 6] = [
        (
            "[Content_Types].xml",
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="bin" ContentType="application/vnd.ms-office.vbaProject"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.ms-excel.sheet.macroEnabled.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        ),
        (
            "xl/workbook.xml",
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.microsoft.com/office/2006/relationships/vbaProject" Target="vbaProject.bin"/></Relationships>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>41</v></c></row></sheetData></worksheet>"#,
        ),
        ("xl/vbaProject.bin", vba),
    ];
    let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    for (name, bytes) in parts {
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Writes the browser test's fixture (`tests/fixtures/vba-hello.xlsm`) and the
/// fuzz seed (`fuzz/seeds/vba/hello.bin`). Run by hand when the builder
/// changes:
/// `cargo test -p casual-calc-import --lib vba::tests::write_fixtures -- --ignored`.
#[test]
#[ignore = "writes fixture files; run by hand when the builder changes"]
fn write_fixtures() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bin = hello_project();
    std::fs::create_dir_all(root.join("fuzz/seeds/vba")).unwrap();
    std::fs::write(root.join("fuzz/seeds/vba/hello.bin"), &bin).unwrap();
    std::fs::create_dir_all(root.join("tests/fixtures")).unwrap();
    std::fs::write(root.join("tests/fixtures/vba-hello.xlsm"), xlsm_with(&bin)).unwrap();
}

/// The committed fixtures are what the builder makes today, so the browser
/// test and the fuzz seed cannot drift from the decoder's own tests.
#[test]
fn the_committed_fixtures_are_the_builders_output() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let seed = std::fs::read(root.join("fuzz/seeds/vba/hello.bin")).unwrap();
    assert_eq!(seed, hello_project(), "re-run write_fixtures");
    let xlsm = std::fs::read(root.join("tests/fixtures/vba-hello.xlsm")).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(xlsm)).unwrap();
    let mut inner = Vec::new();
    std::io::Read::read_to_end(&mut zip.by_name("xl/vbaProject.bin").unwrap(), &mut inner).unwrap();
    assert_eq!(inner, hello_project(), "re-run write_fixtures");
}
