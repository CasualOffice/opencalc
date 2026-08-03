//! Entry-path safety. Rejects traversal, absolute, and drive-qualified paths
//! before any part is read. See `docs/21-PARSER-LIMITS.md`.

/// Returns `true` if `name` is a safe relative package part path.
///
/// Rejects: empty names, absolute paths (`/…` or `\…`), Windows drive-qualified
/// paths (`C:…`), and any path containing a `..` component. Both `/` and `\` are
/// treated as separators so a `\`-escaped path cannot slip through.
pub fn is_safe_part_path(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return false;
    }
    let bytes = name.as_bytes();
    // Windows drive letter, e.g. "C:whatever".
    if bytes.len() >= 2 && bytes[1] == b':' {
        return false;
    }
    for component in name.split(['/', '\\']) {
        if component == ".." {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::is_safe_part_path;

    #[test]
    fn accepts_normal_part_paths() {
        assert!(is_safe_part_path("xl/workbook.xml"));
        assert!(is_safe_part_path("[Content_Types].xml"));
        assert!(is_safe_part_path("xl/worksheets/sheet1.xml"));
    }

    #[test]
    fn rejects_traversal_and_absolute() {
        assert!(!is_safe_part_path(""));
        assert!(!is_safe_part_path("../evil.xml"));
        assert!(!is_safe_part_path("xl/../../etc/passwd"));
        assert!(!is_safe_part_path("/etc/passwd"));
        assert!(!is_safe_part_path("\\windows\\system32"));
        assert!(!is_safe_part_path("C:/Windows/win.ini"));
        assert!(!is_safe_part_path("xl\\..\\secret"));
    }
}
