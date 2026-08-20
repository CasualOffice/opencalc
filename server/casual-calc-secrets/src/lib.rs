//! Reading a secret from the environment, or from a file the environment names.
//!
//! # Why a file at all
//!
//! An environment variable is not a private channel. It is readable in
//! `docker inspect`, in `/proc/1/environ` by anything sharing the namespace, in
//! a crash dump, and in whatever a process manager logs about the processes it
//! starts. A signing key held that way is a signing key that leaks through four
//! mechanisms nobody thinks of as a disclosure — which is `DEP-11`.
//!
//! Every secret store worth using — Kubernetes, Docker's own `secrets:`,
//! systemd's `LoadCredential` — presents a secret as a **file**, and every
//! server worth deploying reads one. The convention this follows is the common
//! one: `FOO_FILE` names a path, and is read instead of `FOO`.
//!
//! # Both is an error, not a preference
//!
//! `FOO` and `FOO_FILE` together are refused rather than ranked. A silent
//! precedence rule is how a deployment that *believes* it moved to files keeps
//! running on the environment variable it forgot to delete — the failure is
//! invisible, and invisible is the whole complaint `DEP-11` makes.

use std::path::PathBuf;

/// Why a secret could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretError {
    /// Both `NAME` and `NAME_FILE` were set.
    Ambiguous {
        /// The variable's name, without the `_FILE` suffix.
        name: String,
    },
    /// `NAME_FILE` names a path that could not be read.
    Unreadable {
        /// The variable's name, without the `_FILE` suffix.
        name: String,
        /// The path it named.
        path: PathBuf,
        /// What the filesystem said.
        why: String,
    },
    /// `NAME_FILE` names a file that holds nothing.
    ///
    /// Refused rather than treated as unset: an empty secret file is a mount
    /// that did not happen, and continuing as though no secret was configured
    /// turns a deployment mistake into a running server with no key.
    Empty {
        /// The variable's name, without the `_FILE` suffix.
        name: String,
        /// The path it named.
        path: PathBuf,
    },
}

impl core::fmt::Display for SecretError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SecretError::Ambiguous { name } => write!(
                f,
                "{name} and {name}_FILE are both set; use one. {name}_FILE is the private \
                 one — {name} is readable in `docker inspect` and /proc/1/environ"
            ),
            SecretError::Unreadable { name, path, why } => {
                write!(
                    f,
                    "{name}_FILE names {path:?}, which could not be read: {why}"
                )
            }
            SecretError::Empty { name, path } => write!(
                f,
                "{name}_FILE names {path:?}, which is empty; a secret file with nothing in \
                 it is a mount that did not happen"
            ),
        }
    }
}

impl std::error::Error for SecretError {}

/// The value of `name`, read from `{name}_FILE` if that is set.
///
/// A trailing newline is stripped, because every way of writing a secret to a
/// file adds one and no secret ends in a newline on purpose. Leading and inner
/// bytes are left exactly as they are: a shared secret may legitimately begin
/// with whitespace, and quietly trimming it produces a key that verifies
/// nothing with no indication why.
///
/// # Errors
///
/// [`SecretError`] when both forms are set, or when the named file cannot be
/// read or holds nothing.
pub fn env_secret(name: &str) -> Result<Option<String>, SecretError> {
    let inline = std::env::var(name).ok().filter(|v| !v.is_empty());
    let from_file = std::env::var(format!("{name}_FILE"))
        .ok()
        .filter(|v| !v.is_empty());
    resolve(name, inline, from_file)
}

/// The decision [`env_secret`] makes, with the environment already read.
///
/// Separated because the environment cannot be written in a test: this
/// workspace is edition 2024 with `unsafe_code = "forbid"`, and
/// `std::env::set_var` is `unsafe` there. A rule that could only be exercised
/// through a process-wide mutation no test may perform would be a rule with no
/// test — so the rule lives here, where a test can reach it.
///
/// # Errors
///
/// [`SecretError`], exactly as [`env_secret`] documents.
pub fn resolve(
    name: &str,
    inline: Option<String>,
    from_file: Option<String>,
) -> Result<Option<String>, SecretError> {
    match (inline, from_file) {
        (Some(_), Some(_)) => Err(SecretError::Ambiguous {
            name: name.to_owned(),
        }),
        (Some(value), None) => Ok(Some(value)),
        (None, Some(path)) => {
            let path = PathBuf::from(path);
            let raw = std::fs::read_to_string(&path).map_err(|why| SecretError::Unreadable {
                name: name.to_owned(),
                path: path.clone(),
                why: why.to_string(),
            })?;
            let value = raw.strip_suffix('\n').unwrap_or(&raw);
            let value = value.strip_suffix('\r').unwrap_or(value);
            if value.is_empty() {
                return Err(SecretError::Empty {
                    name: name.to_owned(),
                    path,
                });
            }
            Ok(Some(value.to_owned()))
        }
        (None, None) => Ok(None),
    }
}

#[cfg(test)]
mod tests;

/// `*_FILE` variables in the environment that no secret here corresponds to.
///
/// A misspelled `OPENCALC_SHARED_SECRETS_FILE` is invisible: the variable is
/// simply never read, the server finds no secret, and the operator is left with
/// a mount that is present, correct, and ignored. Naming the ones that *are*
/// read turns that into a line in the log.
///
/// `known` is each server's literal list, which is also what the deployment
/// page is checked against — so a `_FILE` variable can be documented only if
/// some server names it.
#[must_use]
pub fn unknown_secret_files<S: AsRef<str>>(
    present: impl Iterator<Item = S>,
    known: &[&str],
) -> Vec<String> {
    let mut found: Vec<String> = present
        .filter(|name| {
            let name = name.as_ref();
            name.starts_with("OPENCALC_") && name.ends_with("_FILE") && !known.contains(&name)
        })
        .map(|name| name.as_ref().to_owned())
        .collect();
    found.sort();
    found
}
