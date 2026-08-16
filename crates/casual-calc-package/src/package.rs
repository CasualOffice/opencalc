//! Bounded ZIP/OPC package admission and on-demand part reads.

use std::fmt;
use std::io::{Cursor, Read};

use zip::ZipArchive;

use crate::error::PackageError;
use crate::limits::PackageLimits;
use crate::path::is_safe_part_path;

/// Metadata for one admitted entry (part).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInfo {
    /// The part's path within the package.
    pub name: String,
    /// Declared uncompressed size in bytes.
    pub uncompressed: u64,
    /// Compressed size in bytes.
    pub compressed: u64,
}

/// A security-bounded, admitted ZIP/OPC package.
///
/// [`Package::open`] validates the whole archive against [`PackageLimits`]
/// *before* any part is decompressed; parts are then read on demand and under
/// the same size ceiling via [`Package::read_part`]. This substrate is
/// format-neutral: it backs both `.xlsx` (SpreadsheetML) and `.ods` admission.
pub struct Package {
    archive: ZipArchive<Cursor<Vec<u8>>>,
    limits: PackageLimits,
    /// Bytes actually handed out by [`Package::read_part`] so far.
    ///
    /// The whole-package ceiling is checked at admission against the sizes the
    /// central directory *declares*, which is the only information available
    /// before decompressing anything. That check is worth nothing on its own:
    /// the declaration is attacker-controlled, and a part that says 1,000 and
    /// produces 600 MiB was admitted and then expanded. So the same ceiling is
    /// charged again here, against bytes that actually exist — five parts of
    /// 200 MiB each cannot each pass a 4 GiB test individually and add up to a
    /// gigabyte of retained memory.
    consumed: u64,
}

impl fmt::Debug for Package {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Package")
            .field("entries", &self.archive.len())
            .field("limits", &self.limits)
            .finish()
    }
}

impl Package {
    /// Admit `bytes` as a package under `limits`, or fail cleanly.
    ///
    /// Enforces, in order: input size, entry count, per-entry path safety and
    /// length, cumulative uncompressed size, and overall expansion ratio. On any
    /// breach the package is rejected — never partially admitted.
    pub fn open(bytes: Vec<u8>, limits: PackageLimits) -> Result<Package, PackageError> {
        let input_len = bytes.len() as u64;
        if input_len > limits.max_input_bytes {
            return Err(PackageError::InputTooLarge {
                size: input_len,
                limit: limits.max_input_bytes,
            });
        }

        let mut archive =
            ZipArchive::new(Cursor::new(bytes)).map_err(|_| PackageError::NotAPackage)?;

        let count = archive.len();
        if count > limits.max_entries {
            return Err(PackageError::TooManyEntries {
                count,
                limit: limits.max_entries,
            });
        }

        let mut total_uncompressed: u64 = 0;
        let mut total_compressed: u64 = 0;

        for i in 0..count {
            // Reading metadata does not decompress the entry.
            let entry = archive.by_index(i).map_err(|_| PackageError::NotAPackage)?;
            let name = entry.name().to_owned();

            if name.len() > limits.max_path_bytes {
                return Err(PackageError::PathTooLong {
                    len: name.len(),
                    limit: limits.max_path_bytes,
                });
            }
            if !entry.is_dir() && !is_safe_part_path(&name) {
                return Err(PackageError::UnsafePath { path: name });
            }

            total_uncompressed = total_uncompressed.saturating_add(entry.size());
            total_compressed = total_compressed.saturating_add(entry.compressed_size());

            if total_uncompressed > limits.max_total_uncompressed {
                return Err(PackageError::ExpansionTooLarge {
                    total: total_uncompressed,
                    limit: limits.max_total_uncompressed,
                });
            }
        }

        if let Some(ratio) = total_uncompressed.checked_div(total_compressed)
            && ratio > limits.max_expansion_ratio
        {
            return Err(PackageError::ExpansionRatioExceeded {
                ratio,
                limit: limits.max_expansion_ratio,
            });
        }

        Ok(Package {
            archive,
            limits,
            consumed: 0,
        })
    }

    /// The number of entries in the package.
    pub fn len(&self) -> usize {
        self.archive.len()
    }

    /// Whether the package has no entries.
    pub fn is_empty(&self) -> bool {
        self.archive.is_empty()
    }

    /// The paths of all entries, in archive order.
    pub fn entry_names(&self) -> Vec<String> {
        self.archive.file_names().map(str::to_owned).collect()
    }

    /// Metadata for every entry, in archive order.
    pub fn entries(&mut self) -> Result<Vec<EntryInfo>, PackageError> {
        let count = self.archive.len();
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let entry = self
                .archive
                .by_index(i)
                .map_err(|_| PackageError::NotAPackage)?;
            out.push(EntryInfo {
                name: entry.name().to_owned(),
                uncompressed: entry.size(),
                compressed: entry.compressed_size(),
            });
        }
        Ok(out)
    }

    /// Whether a part with the given path exists.
    pub fn contains(&self, name: &str) -> bool {
        self.archive.file_names().any(|n| n == name)
    }

    /// Read a part's decompressed bytes, bounded by what it declared and by
    /// what the package as a whole has already produced.
    ///
    /// **The declared size is the bound, not a hint.** [`Package::open`] adds
    /// up `entry.size()` across the archive and refuses a package whose total
    /// or expansion ratio is too high — but every one of those numbers comes
    /// from the central directory, which is part of the attacker's input.
    /// Patching one four-byte field turned a 1 MB file that was correctly
    /// rejected as a 1028:1 bomb into one that was admitted and then handed
    /// back 600 MiB. The admission check is only meaningful if the bytes are
    /// held to the declaration it was computed from, so a part that produces
    /// more than it said is refused here.
    ///
    /// The whole-package ceiling is charged a second time, cumulatively, for
    /// the same reason: each part staying under a 4 GiB cap says nothing about
    /// five of them together, and unmodelled parts are *retained* in the model,
    /// so the bytes stay resident.
    pub fn read_part(&mut self, name: &str) -> Result<Vec<u8>, PackageError> {
        let total_cap = self.limits.max_total_uncompressed;
        let remaining = total_cap.saturating_sub(self.consumed);
        let mut entry = self
            .archive
            .by_name(name)
            .map_err(|_| PackageError::PartNotFound {
                name: name.to_owned(),
            })?;

        // Whichever is tighter: what this entry claims it will produce, or what
        // the package has left in its budget.
        let declared = entry.size();
        let cap = declared.min(remaining);
        let hint = usize::try_from(cap).unwrap_or(usize::MAX);
        let mut buf = Vec::with_capacity(hint);
        // One byte past the cap, so exceeding it is detectable rather than
        // silently truncated — a truncated part is a corrupt document that
        // reports success.
        entry
            .by_ref()
            .take(cap.saturating_add(1))
            .read_to_end(&mut buf)
            .map_err(|_| PackageError::NotAPackage)?;

        let produced = buf.len() as u64;
        if produced > cap {
            return Err(PackageError::ExpansionTooLarge {
                total: self.consumed.saturating_add(produced),
                limit: total_cap,
            });
        }
        self.consumed = self.consumed.saturating_add(produced);
        Ok(buf)
    }
}
