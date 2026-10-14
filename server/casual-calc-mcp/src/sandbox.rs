//! The filesystem roots an agent may read and write under (docs/97, security
//! bounds).

use std::fmt;
use std::path::{Path, PathBuf};

/// Why a path was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxError {
    /// The server was started without `--root`, so no file is reachable.
    NoRoots,
    /// The path resolves outside every root.
    Outside(String),
    /// The path could not be resolved (missing file, missing directory).
    Unresolvable(String, String),
    /// The path does not name a file.
    NotAFile(String),
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRoots => f.write_str("no file access: the server was started without --root"),
            Self::Outside(p) => write!(f, "{p} is outside every allowed root"),
            Self::Unresolvable(p, why) => write!(f, "cannot resolve {p}: {why}"),
            Self::NotAFile(p) => write!(f, "{p} is not a file"),
        }
    }
}

/// The set of allowed roots.
#[derive(Debug, Clone, Default)]
pub struct Sandbox {
    roots: Vec<PathBuf>,
}

impl Sandbox {
    /// A sandbox over `roots`.
    ///
    /// # Errors
    ///
    /// A root that does not exist or is not a directory.
    pub fn new(roots: &[PathBuf]) -> Result<Self, String> {
        let mut canonical = Vec::with_capacity(roots.len());
        for root in roots {
            // Canonicalised once, here: every later check compares canonical
            // paths, so a root given through a symlink (`/tmp` on macOS is
            // `/private/tmp`) must be compared in its resolved form too.
            let resolved = root
                .canonicalize()
                .map_err(|e| format!("--root {}: {e}", root.display()))?;
            if !resolved.is_dir() {
                return Err(format!("--root {}: not a directory", root.display()));
            }
            canonical.push(resolved);
        }
        Ok(Self { roots: canonical })
    }

    /// The canonical roots.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// `path` made absolute: a relative path is taken relative to the first
    /// root, which is the only reading that cannot depend on the directory the
    /// server happened to be launched from.
    fn absolute(&self, path: &str) -> Result<PathBuf, SandboxError> {
        let first = self.roots.first().ok_or(SandboxError::NoRoots)?;
        let p = Path::new(path);
        Ok(if p.is_absolute() {
            p.to_path_buf()
        } else {
            first.join(p)
        })
    }

    /// Whether a **canonical** path lies under a root. Component-wise
    /// (`Path::starts_with`), so `/data/root2` is not inside `/data/root`.
    fn inside(&self, canonical: &Path) -> bool {
        self.roots.iter().any(|root| canonical.starts_with(root))
    }

    /// Resolve an existing file to read.
    ///
    /// The path is canonicalised — every `..` and every symlink, at any
    /// depth, resolved by the operating system — and the *result* must lie
    /// under a root. Checking the path as written would let `root/link`
    /// pointing at `/etc` through.
    ///
    /// # Errors
    ///
    /// See [`SandboxError`].
    pub fn resolve_existing(&self, path: &str) -> Result<PathBuf, SandboxError> {
        let absolute = self.absolute(path)?;
        let canonical = absolute
            .canonicalize()
            .map_err(|e| SandboxError::Unresolvable(path.to_owned(), e.to_string()))?;
        if !self.inside(&canonical) {
            return Err(SandboxError::Outside(path.to_owned()));
        }
        if !canonical.is_file() {
            return Err(SandboxError::NotAFile(path.to_owned()));
        }
        Ok(canonical)
    }

    /// Resolve a path to write, which need not exist yet.
    ///
    /// The **directory** must exist and is canonicalised, so `..` and a
    /// symlinked directory are resolved before the root check. If the file
    /// itself exists and is a symlink, its target is what gets written, so
    /// the target must resolve and lie under a root as well; a dangling
    /// symlink is refused, because creating its target would create a file
    /// wherever the link points.
    ///
    /// # Errors
    ///
    /// See [`SandboxError`].
    pub fn resolve_for_write(&self, path: &str) -> Result<PathBuf, SandboxError> {
        let absolute = self.absolute(path)?;
        // `file_name` is `None` for a path ending in `..` or `/`, neither of
        // which names a file.
        let name = absolute
            .file_name()
            .ok_or_else(|| SandboxError::NotAFile(path.to_owned()))?;
        let parent = absolute
            .parent()
            .ok_or_else(|| SandboxError::NotAFile(path.to_owned()))?;
        let parent = parent
            .canonicalize()
            .map_err(|e| SandboxError::Unresolvable(path.to_owned(), e.to_string()))?;
        if !self.inside(&parent) {
            return Err(SandboxError::Outside(path.to_owned()));
        }
        let candidate = parent.join(name);
        let target = match std::fs::symlink_metadata(&candidate) {
            Err(_) => candidate,
            Ok(meta) if meta.file_type().is_symlink() => {
                let resolved = candidate.canonicalize().map_err(|_| {
                    SandboxError::Unresolvable(
                        path.to_owned(),
                        "it is a symlink to nothing".to_owned(),
                    )
                })?;
                if !self.inside(&resolved) {
                    return Err(SandboxError::Outside(path.to_owned()));
                }
                resolved
            }
            Ok(_) => candidate,
        };
        if target.is_dir() {
            return Err(SandboxError::NotAFile(path.to_owned()));
        }
        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A scratch tree: `<base>/root` (the allowed root) and `<base>/outside`.
    struct Tree {
        base: PathBuf,
    }

    impl Tree {
        fn new() -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let base = std::env::temp_dir().join(format!(
                "casual-calc-mcp-sandbox-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(base.join("root/sub")).unwrap();
            std::fs::create_dir_all(base.join("outside")).unwrap();
            std::fs::write(base.join("root/in.xlsx"), b"in").unwrap();
            std::fs::write(base.join("root/sub/deep.csv"), b"deep").unwrap();
            std::fs::write(base.join("outside/secret.xlsx"), b"secret").unwrap();
            Self { base }
        }
        fn root(&self) -> PathBuf {
            self.base.join("root")
        }
        fn p(&self, rel: &str) -> String {
            self.base.join(rel).to_string_lossy().into_owned()
        }
        fn sandbox(&self) -> Sandbox {
            Sandbox::new(&[self.root()]).unwrap()
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    #[cfg(unix)]
    fn symlink(target: &Path, link: &Path) {
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[test]
    fn a_file_inside_the_root_resolves_to_its_canonical_path() {
        let t = Tree::new();
        let got = t.sandbox().resolve_existing(&t.p("root/in.xlsx")).unwrap();
        assert_eq!(got, t.base.join("root/in.xlsx").canonicalize().unwrap());
        let deep = t.sandbox().resolve_existing(&t.p("root/sub/deep.csv"));
        assert!(deep.is_ok(), "{deep:?}");
    }

    #[test]
    fn a_relative_path_is_taken_relative_to_the_first_root() {
        let t = Tree::new();
        let got = t.sandbox().resolve_existing("sub/deep.csv").unwrap();
        assert_eq!(
            got,
            t.base.join("root/sub/deep.csv").canonicalize().unwrap()
        );
        assert!(matches!(
            t.sandbox().resolve_existing("../outside/secret.xlsx"),
            Err(SandboxError::Outside(_))
        ));
    }

    #[test]
    fn dot_dot_out_of_the_root_is_refused() {
        let t = Tree::new();
        let escape = t.p("root/../outside/secret.xlsx");
        assert!(
            matches!(
                t.sandbox().resolve_existing(&escape),
                Err(SandboxError::Outside(_))
            ),
            "read through .. escaped the root"
        );
        let write = t.p("root/sub/../../outside/new.xlsx");
        assert!(
            matches!(
                t.sandbox().resolve_for_write(&write),
                Err(SandboxError::Outside(_))
            ),
            "write through .. escaped the root"
        );
    }

    #[test]
    fn an_absolute_path_elsewhere_is_refused() {
        let t = Tree::new();
        assert!(matches!(
            t.sandbox().resolve_existing(&t.p("outside/secret.xlsx")),
            Err(SandboxError::Outside(_))
        ));
        assert!(matches!(
            t.sandbox().resolve_for_write(&t.p("outside/new.xlsx")),
            Err(SandboxError::Outside(_))
        ));
    }

    #[test]
    fn a_sibling_whose_name_extends_the_root_is_not_inside_it() {
        let t = Tree::new();
        std::fs::create_dir_all(t.base.join("root2")).unwrap();
        std::fs::write(t.base.join("root2/x.xlsx"), b"x").unwrap();
        assert!(matches!(
            t.sandbox().resolve_existing(&t.p("root2/x.xlsx")),
            Err(SandboxError::Outside(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_root_is_refused_for_reading_and_writing() {
        let t = Tree::new();
        symlink(
            &t.base.join("outside/secret.xlsx"),
            &t.base.join("root/link.xlsx"),
        );
        symlink(&t.base.join("outside"), &t.base.join("root/linkdir"));
        let s = t.sandbox();
        assert!(
            matches!(
                s.resolve_existing(&t.p("root/link.xlsx")),
                Err(SandboxError::Outside(_))
            ),
            "read through a file symlink escaped the root"
        );
        assert!(
            matches!(
                s.resolve_existing(&t.p("root/linkdir/secret.xlsx")),
                Err(SandboxError::Outside(_))
            ),
            "read through a directory symlink escaped the root"
        );
        assert!(
            matches!(
                s.resolve_for_write(&t.p("root/link.xlsx")),
                Err(SandboxError::Outside(_))
            ),
            "overwriting a symlink would write outside the root"
        );
        assert!(
            matches!(
                s.resolve_for_write(&t.p("root/linkdir/new.xlsx")),
                Err(SandboxError::Outside(_))
            ),
            "a new file under a symlinked directory lands outside the root"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_symlink_is_refused_as_a_write_target() {
        let t = Tree::new();
        symlink(
            &t.base.join("outside/not-yet.xlsx"),
            &t.base.join("root/dangling.xlsx"),
        );
        assert!(
            t.sandbox()
                .resolve_for_write(&t.p("root/dangling.xlsx"))
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_that_stays_inside_the_root_is_allowed() {
        let t = Tree::new();
        symlink(
            &t.base.join("root/in.xlsx"),
            &t.base.join("root/alias.xlsx"),
        );
        let got = t
            .sandbox()
            .resolve_existing(&t.p("root/alias.xlsx"))
            .unwrap();
        assert_eq!(got, t.base.join("root/in.xlsx").canonicalize().unwrap());
    }

    #[test]
    fn a_new_file_inside_the_root_may_be_written() {
        let t = Tree::new();
        let got = t
            .sandbox()
            .resolve_for_write(&t.p("root/sub/out.xlsx"))
            .unwrap();
        assert_eq!(
            got,
            t.base
                .join("root/sub")
                .canonicalize()
                .unwrap()
                .join("out.xlsx")
        );
    }

    #[test]
    fn a_write_into_a_missing_directory_or_onto_a_directory_is_refused() {
        let t = Tree::new();
        assert!(
            t.sandbox()
                .resolve_for_write(&t.p("root/nope/out.xlsx"))
                .is_err()
        );
        assert!(t.sandbox().resolve_for_write(&t.p("root/sub")).is_err());
        assert!(t.sandbox().resolve_for_write(&t.p("root/sub/..")).is_err());
    }

    #[test]
    fn with_no_roots_every_path_is_refused() {
        let t = Tree::new();
        let none = Sandbox::new(&[]).unwrap();
        assert_eq!(
            none.resolve_existing(&t.p("root/in.xlsx")),
            Err(SandboxError::NoRoots)
        );
        assert_eq!(
            none.resolve_for_write(&t.p("root/out.xlsx")),
            Err(SandboxError::NoRoots)
        );
    }

    #[test]
    fn a_root_that_does_not_exist_is_a_startup_error() {
        let t = Tree::new();
        assert!(Sandbox::new(&[t.base.join("missing")]).is_err());
        assert!(Sandbox::new(&[t.base.join("root/in.xlsx")]).is_err());
    }

    #[test]
    fn reading_a_directory_is_refused() {
        let t = Tree::new();
        assert!(matches!(
            t.sandbox().resolve_existing(&t.p("root/sub")),
            Err(SandboxError::NotAFile(_))
        ));
    }
}
