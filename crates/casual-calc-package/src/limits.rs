//! Non-bypassable admission limits. See `docs/21-PARSER-LIMITS.md`.

/// Hard ceilings applied when admitting a package. Every field bounds an axis a
/// hostile archive could abuse (total size, entry count, expansion, path
/// length). Construct with [`PackageLimits::default`] and tighten as needed; the
/// defaults are the documented package ceilings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageLimits {
    /// Maximum accepted input (compressed archive) size, in bytes.
    pub max_input_bytes: u64,
    /// Maximum number of entries (parts) in the archive.
    pub max_entries: usize,
    /// Maximum total *uncompressed* size across all entries, in bytes.
    pub max_total_uncompressed: u64,
    /// Maximum uncompressed:compressed ratio (zip-bomb defense).
    pub max_expansion_ratio: u64,
    /// Maximum length of any entry path, in bytes.
    pub max_path_bytes: usize,
}

impl Default for PackageLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 1 << 30, // 1 GiB
            max_entries: 50_000,
            max_total_uncompressed: 4u64 << 30, // 4 GiB
            max_expansion_ratio: 1000,
            max_path_bytes: 4096,
        }
    }
}
