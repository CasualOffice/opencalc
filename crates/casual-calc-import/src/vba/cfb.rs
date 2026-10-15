//! A read-only reader for the **Compound File Binary** container (`[MS-CFB]`),
//! the "file system in a file" a `vbaProject.bin` is written in.
//!
//! Only what the VBA viewer needs: the header, the FAT (with its DIFAT), the
//! MiniFAT and mini stream, the directory, and reading a stream by path. No
//! writing, no transaction or property-set handling.
//!
//! # Hostile input
//!
//! The bytes come from a file somebody else wrote. Every number in them —
//! a sector id, a chain link, a stream size, a directory index — is a claim to
//! be checked, not a fact:
//!
//! - every sector chain is followed with a **visited set**, so a FAT that loops
//!   is [`VbaError::ChainCycle`] rather than a hang;
//! - a stream's claimed size is checked against what the container *could*
//!   hold before anything is allocated for it ([`VbaError::OversizedStream`]),
//!   so a four-byte claim cannot reserve four gigabytes;
//! - every table is sized from the input's length, never from a count the
//!   input states, so allocation is linear in the bytes actually given;
//! - directory-tree walks carry a visited set too, because sibling and child
//!   links can point anywhere, including back up the tree.

use super::VbaError;

const SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const HEADER_LEN: usize = 512;
const DIR_ENTRY_LEN: usize = 128;
/// `[MS-CFB]` §2.2: the size below which a stream lives in the mini stream. It
/// is in the header too, but the spec fixes it at 4096 and a reader that took
/// the header's word would let a hostile file move streams between the two
/// allocation tables at will.
const MINI_STREAM_CUTOFF: u64 = 4096;
const MINI_SECTOR_LEN: usize = 64;
/// Header's in-line DIFAT: the first 109 FAT sector ids.
const HEADER_DIFAT: usize = 109;

/// Special sector ids.
const ENDOFCHAIN: u32 = 0xFFFF_FFFE;
const FREESECT: u32 = 0xFFFF_FFFF;
/// `NOSTREAM` in a directory link.
const NOSTREAM: u32 = 0xFFFF_FFFF;
/// The largest sector id that names a real sector (`MAXREGSECT`).
const MAXREGSECT: u32 = 0xFFFF_FFFA;

/// Object types in a directory entry.
const TYPE_STREAM: u8 = 2;
const TYPE_ROOT: u8 = 5;

/// One directory entry, as much of it as the reader uses.
#[derive(Debug, Clone)]
struct DirEntry {
    name: String,
    kind: u8,
    left: u32,
    right: u32,
    child: u32,
    start: u32,
    size: u64,
}

/// An opened compound file: the bytes plus the tables needed to read streams.
#[derive(Debug)]
pub(crate) struct CompoundFile<'a> {
    bytes: &'a [u8],
    sector_len: usize,
    fat: Vec<u32>,
    minifat: Vec<u32>,
    mini_stream: Vec<u8>,
    entries: Vec<DirEntry>,
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, VbaError> {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or(VbaError::Truncated("compound file header"))
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, VbaError> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or(VbaError::Truncated("compound file header"))
}

impl<'a> CompoundFile<'a> {
    /// Parse the header and the tables. Reads no stream content.
    pub(crate) fn open(bytes: &'a [u8]) -> Result<Self, VbaError> {
        if bytes.len() < HEADER_LEN || bytes[..8] != SIGNATURE {
            return Err(VbaError::NotCompoundFile);
        }
        let major = u16_at(bytes, 0x1A)?;
        let shift = u16_at(bytes, 0x1E)?;
        // Version 3 is 512-byte sectors, version 4 is 4096. Nothing else exists,
        // and accepting another shift would let `1 << shift` be anything.
        let sector_len = match (major, shift) {
            (3, 9) => 512,
            (4, 12) => 4096,
            _ => return Err(VbaError::UnsupportedSectorSize(shift)),
        };
        if u16_at(bytes, 0x20)? != 6 {
            return Err(VbaError::UnsupportedSectorSize(shift));
        }
        let num_fat = u32_at(bytes, 0x2C)? as usize;
        let first_dir = u32_at(bytes, 0x30)?;
        let first_minifat = u32_at(bytes, 0x3C)?;
        let first_difat = u32_at(bytes, 0x44)?;
        let num_difat = u32_at(bytes, 0x48)? as usize;

        // The number of sectors the file can actually hold bounds every table.
        let sectors_in_file = bytes.len().saturating_sub(sector_len) / sector_len + 1;
        if num_fat > sectors_in_file {
            return Err(VbaError::Truncated("FAT"));
        }

        let mut cf = Self {
            bytes,
            sector_len,
            fat: Vec::new(),
            minifat: Vec::new(),
            mini_stream: Vec::new(),
            entries: Vec::new(),
        };

        // ---- FAT sector ids: the header's 109, then the DIFAT chain. ----
        let mut fat_sectors: Vec<u32> = Vec::with_capacity(num_fat);
        for i in 0..HEADER_DIFAT.min(num_fat) {
            fat_sectors.push(u32_at(bytes, 0x4C + 4 * i)?);
        }
        let per_difat = sector_len / 4 - 1;
        let mut next = first_difat;
        let mut seen_difat = 0usize;
        while fat_sectors.len() < num_fat {
            if next > MAXREGSECT || seen_difat >= num_difat.min(sectors_in_file) {
                return Err(VbaError::Truncated("DIFAT"));
            }
            seen_difat += 1;
            let sector = cf.sector(next)?;
            for i in 0..per_difat {
                if fat_sectors.len() == num_fat {
                    break;
                }
                fat_sectors.push(u32_at(sector, 4 * i)?);
            }
            next = u32_at(sector, 4 * per_difat)?;
        }

        // ---- The FAT itself. ----
        let mut fat = Vec::with_capacity(num_fat * (sector_len / 4));
        for &id in &fat_sectors {
            let sector = cf.sector(id)?;
            fat.extend(
                sector
                    .chunks_exact(4)
                    .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])),
            );
        }
        cf.fat = fat;

        // ---- Directory. ----
        let dir_bytes = cf.chain(first_dir, None, "directory")?;
        let mut entries = Vec::with_capacity(dir_bytes.len() / DIR_ENTRY_LEN);
        for raw in dir_bytes.chunks_exact(DIR_ENTRY_LEN) {
            entries.push(parse_entry(raw, major)?);
        }
        if entries.first().map(|e| e.kind) != Some(TYPE_ROOT) {
            return Err(VbaError::BadDirectory("the first entry is not the root"));
        }
        cf.entries = entries;

        // ---- MiniFAT and the mini stream (held by the root entry). ----
        if first_minifat != ENDOFCHAIN && first_minifat != FREESECT {
            let raw = cf.chain(first_minifat, None, "MiniFAT")?;
            cf.minifat = raw
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
        }
        let root = cf.entries[0].clone();
        if root.size > 0 {
            cf.mini_stream = cf.regular_stream(root.start, root.size)?;
        }
        Ok(cf)
    }

    /// The bytes of sector `id`. The last sector of a file may be short — some
    /// writers do not pad it — so a partial final sector is returned as it is.
    fn sector(&self, id: u32) -> Result<&'a [u8], VbaError> {
        if id > MAXREGSECT {
            return Err(VbaError::BadSector(id));
        }
        let start = (id as usize)
            .checked_add(1)
            .and_then(|n| n.checked_mul(self.sector_len))
            .ok_or(VbaError::BadSector(id))?;
        if start >= self.bytes.len() {
            return Err(VbaError::BadSector(id));
        }
        let end = (start + self.sector_len).min(self.bytes.len());
        Ok(&self.bytes[start..end])
    }

    /// Follow a FAT chain from `start`, concatenating its sectors. With `limit`,
    /// stops once that many bytes are read. A chain that revisits a sector is a
    /// cycle; one that names a sector the FAT does not have is out of range.
    fn chain(
        &self,
        start: u32,
        limit: Option<usize>,
        what: &'static str,
    ) -> Result<Vec<u8>, VbaError> {
        let mut out = Vec::new();
        let mut visited = vec![false; self.fat.len()];
        let mut id = start;
        while id != ENDOFCHAIN {
            if limit.is_some_and(|l| out.len() >= l) {
                break;
            }
            let slot = visited
                .get_mut(id as usize)
                .ok_or(VbaError::BadSector(id))?;
            if *slot {
                return Err(VbaError::ChainCycle);
            }
            *slot = true;
            out.extend_from_slice(self.sector(id)?);
            id = self.fat[id as usize];
        }
        if let Some(l) = limit {
            if out.len() < l {
                return Err(VbaError::Truncated(what));
            }
            out.truncate(l);
        }
        Ok(out)
    }

    fn regular_stream(&self, start: u32, size: u64) -> Result<Vec<u8>, VbaError> {
        // A size the file cannot hold is refused before anything is allocated.
        if size > self.bytes.len() as u64 {
            return Err(VbaError::OversizedStream(size));
        }
        self.chain(start, Some(size as usize), "stream")
    }

    fn mini_stream_read(&self, start: u32, size: u64) -> Result<Vec<u8>, VbaError> {
        if size > self.mini_stream.len() as u64 {
            return Err(VbaError::OversizedStream(size));
        }
        let size = size as usize;
        let mut out = Vec::with_capacity(size);
        let mut visited = vec![false; self.minifat.len()];
        let mut id = start;
        while out.len() < size {
            if id == ENDOFCHAIN {
                return Err(VbaError::Truncated("mini stream"));
            }
            let slot = visited
                .get_mut(id as usize)
                .ok_or(VbaError::BadSector(id))?;
            if *slot {
                return Err(VbaError::ChainCycle);
            }
            *slot = true;
            let at = (id as usize) * MINI_SECTOR_LEN;
            let sector = self
                .mini_stream
                .get(at..(at + MINI_SECTOR_LEN).min(self.mini_stream.len()))
                .filter(|s| !s.is_empty())
                .ok_or(VbaError::BadSector(id))?;
            out.extend_from_slice(sector);
            id = self.minifat[id as usize];
        }
        out.truncate(size);
        Ok(out)
    }

    /// The direct children of storage entry `parent`, walking its red-black
    /// sibling tree. Indices only; a link that leaves the table is an error, a
    /// link that revisits an entry is a cycle.
    fn children(&self, parent: usize) -> Result<Vec<usize>, VbaError> {
        let mut out = Vec::new();
        let mut visited = vec![false; self.entries.len()];
        let mut stack = vec![self.entries[parent].child];
        while let Some(link) = stack.pop() {
            if link == NOSTREAM {
                continue;
            }
            let i = link as usize;
            let slot = visited
                .get_mut(i)
                .ok_or(VbaError::BadDirectory("a link leaves the directory"))?;
            if *slot || i == 0 {
                return Err(VbaError::BadDirectory("the directory tree has a cycle"));
            }
            *slot = true;
            out.push(i);
            stack.push(self.entries[i].left);
            stack.push(self.entries[i].right);
        }
        Ok(out)
    }

    /// The entry at `path` (`/`-separated, compared case-insensitively the way
    /// `[MS-CFB]` compares names), or `None` if there is none.
    fn find(&self, path: &str) -> Result<Option<usize>, VbaError> {
        let mut at = 0usize;
        for part in path.split('/') {
            let want = part.to_uppercase();
            let mut found = None;
            for i in self.children(at)? {
                if self.entries[i].name.to_uppercase() == want {
                    found = Some(i);
                    break;
                }
            }
            match found {
                Some(i) => at = i,
                None => return Ok(None),
            }
        }
        Ok(Some(at))
    }

    /// The content of the stream at `path`, or `None` when there is no such
    /// entry. An entry that exists but is a storage is `None` too.
    pub(crate) fn stream(&self, path: &str) -> Result<Option<Vec<u8>>, VbaError> {
        match self.find(path)? {
            Some(i) => self.read(i),
            None => Ok(None),
        }
    }

    /// The streams directly inside the storage at `path`, as
    /// `(upper-cased name, entry)` — listed once so a project with many modules
    /// does not walk the directory tree once per module.
    pub(crate) fn streams_in(&self, path: &str) -> Result<Vec<(String, usize)>, VbaError> {
        let Some(at) = self.find(path)? else {
            return Ok(Vec::new());
        };
        Ok(self
            .children(at)?
            .into_iter()
            .filter(|&i| self.entries[i].kind == TYPE_STREAM)
            .map(|i| (self.entries[i].name.to_uppercase(), i))
            .collect())
    }

    /// The content of stream entry `i`; `None` if it is not a stream.
    pub(crate) fn read(&self, i: usize) -> Result<Option<Vec<u8>>, VbaError> {
        let Some(entry) = self.entries.get(i) else {
            return Ok(None);
        };
        if entry.kind != TYPE_STREAM {
            return Ok(None);
        }
        if entry.size < MINI_STREAM_CUTOFF {
            self.mini_stream_read(entry.start, entry.size).map(Some)
        } else {
            self.regular_stream(entry.start, entry.size).map(Some)
        }
    }
}

fn parse_entry(raw: &[u8], major: u16) -> Result<DirEntry, VbaError> {
    let name_len = (u16_at(raw, 0x40)? as usize).min(64);
    // The length counts the terminating NUL, in bytes.
    let units: Vec<u16> = raw[..name_len.saturating_sub(2)]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let name = String::from_utf16_lossy(&units);
    let size_raw = u64::from(u32_at(raw, 0x78)?) | (u64::from(u32_at(raw, 0x7C)?) << 32);
    // `[MS-CFB]` §2.6.3: in a version 3 file the high half "SHOULD be zero",
    // and readers ignore it — some writers leave junk there.
    let size = if major == 3 {
        size_raw & 0xFFFF_FFFF
    } else {
        size_raw
    };
    Ok(DirEntry {
        name,
        kind: raw[0x42],
        left: u32_at(raw, 0x44)?,
        right: u32_at(raw, 0x48)?,
        child: u32_at(raw, 0x4C)?,
        start: u32_at(raw, 0x74)?,
        size,
    })
}
