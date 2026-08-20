//! What a deployment gets wrong, and what it must be told.

use std::io::Write as _;

use super::*;

/// A file with `body` in it, beside the test's own temporary directory.
fn secret_file(name: &str, body: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("opencalc-secrets-{name}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("secret");
    let mut f = std::fs::File::create(&path).expect("create");
    f.write_all(body).expect("write");
    path
}

/// **The point of the row: a mounted secret file is read.**
#[test]
fn a_mounted_file_is_read() {
    let path = secret_file("read", b"a-real-signing-key");
    let got = resolve(
        "OPENCALC_SHARED_SECRET",
        None,
        Some(path.display().to_string()),
    );
    assert_eq!(got, Ok(Some("a-real-signing-key".to_owned())));
}

/// Every way of writing a file adds a newline, and no secret ends in one.
#[test]
fn a_trailing_newline_is_not_part_of_the_secret() {
    for (label, body) in [("lf", "key\n"), ("crlf", "key\r\n")] {
        let path = secret_file(label, body.as_bytes());
        assert_eq!(
            resolve("X", None, Some(path.display().to_string())),
            Ok(Some("key".to_owned())),
            "{label}"
        );
    }
}

/// Only *one* trailing newline, and nothing else is touched: a secret may
/// legitimately contain or end in other whitespace, and trimming it produces a
/// key that verifies nothing with no indication why.
#[test]
fn nothing_but_the_final_newline_is_stripped() {
    let path = secret_file("inner", b"  spaced key \n\n");
    assert_eq!(
        resolve("X", None, Some(path.display().to_string())),
        Ok(Some("  spaced key \n".to_owned()))
    );
}

/// An empty file is a mount that did not happen. Continuing as though nothing
/// was configured is how a server ends up running with no key at all.
#[test]
fn an_empty_file_is_refused_rather_than_treated_as_unset() {
    let path = secret_file("empty", b"\n");
    assert!(matches!(
        resolve(
            "OPENCALC_SHARED_SECRET",
            None,
            Some(path.display().to_string())
        ),
        Err(SecretError::Empty { .. })
    ));
}

/// A misspelled path must stop the server, not fall back to the environment.
#[test]
fn an_unreadable_file_is_refused() {
    let why = resolve(
        "OPENCALC_SHARED_SECRET",
        None,
        Some("/nowhere/at/all".into()),
    )
    .expect_err("a path that does not exist");
    assert!(matches!(why, SecretError::Unreadable { .. }));
    assert!(
        why.to_string().contains("/nowhere/at/all"),
        "the operator is not told which path: {why}"
    );
}

/// **Both set is an error, not a precedence rule.**
///
/// A deployment that believes it moved to files, and left the variable behind,
/// otherwise keeps running on the variable — silently, which is the whole
/// complaint the row makes.
#[test]
fn setting_both_is_refused() {
    let path = secret_file("both", b"from-the-file");
    let why = resolve(
        "OPENCALC_SHARED_SECRET",
        Some("from-the-environment".into()),
        Some(path.display().to_string()),
    )
    .expect_err("both forms set");
    assert_eq!(
        why,
        SecretError::Ambiguous {
            name: "OPENCALC_SHARED_SECRET".to_owned()
        }
    );
    assert!(
        why.to_string().contains("docker inspect"),
        "the operator is not told which one to keep: {why}"
    );
}

/// The environment still works, so this is not a breaking change.
#[test]
fn the_environment_variable_still_works() {
    assert_eq!(
        resolve("X", Some("plain".into()), None),
        Ok(Some("plain".to_owned()))
    );
    assert_eq!(resolve("X", None, None), Ok(None));
}

/// A misspelled secret file is a mount that is present, correct and ignored.
#[test]
fn a_misspelled_secret_file_is_named() {
    let known = ["OPENCALC_SHARED_SECRET_FILE", "OPENCALC_REDIS_URL_FILE"];
    let present = [
        "OPENCALC_SHARED_SECRET_FILE",  // read
        "OPENCALC_SHARED_SECRETS_FILE", // the plural typo, read by nothing
        "OPENCALC_BIND",                // not a file
        "PATH",                         // not ours
        "OPENCALC_ADMIN_TOKEN_FILE",    // a real name, but not this server's
    ];
    assert_eq!(
        unknown_secret_files(present.iter(), &known),
        vec![
            "OPENCALC_ADMIN_TOKEN_FILE".to_owned(),
            "OPENCALC_SHARED_SECRETS_FILE".to_owned(),
        ]
    );
}

/// And a correctly-spelled environment says nothing.
#[test]
fn a_correct_environment_produces_no_complaint() {
    let known = ["OPENCALC_SHARED_SECRET_FILE"];
    assert!(
        unknown_secret_files(["OPENCALC_SHARED_SECRET_FILE", "HOME"].iter(), &known).is_empty()
    );
}
