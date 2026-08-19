use super::*;

fn info(can_write: bool, can_update: bool) -> FileInfo {
    FileInfo {
        base_file_name: "Q3.xlsx".to_owned(),
        user_friendly_name: Some("Ada".to_owned()),
        user_id: Some("u-7".to_owned()),
        user_can_write: can_write,
        supports_locks: true,
        supports_update: can_update,
    }
}

/// **A session id is unguessable, and no two are alike.**
///
/// It is the capability to read and overwrite somebody's file. The demo host
/// mints its ids from a hash of the clock and says in its own comment that this
/// is "enough for a demo" — an id built that way here would let anyone who can
/// see one session's URL derive another's.
#[test]
fn session_ids_are_random_and_long() {
    let ids: std::collections::HashSet<String> = (0..500).map(|_| fresh_id(32)).collect();
    assert_eq!(ids.len(), 500, "ids repeated");
    for id in &ids {
        assert_eq!(id.len(), 64, "32 bytes, hex encoded");
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

/// **A host that cannot be saved to opens read-only, even if it says the user
/// may write.**
///
/// `UserCanWrite` and `SupportsUpdate` are different claims, and a host can
/// make the first without the second. Believing only the first means the
/// session looks editable for an hour and fails at the save — the worst moment
/// to find out, because by then there is work to lose.
#[test]
fn writing_needs_both_permission_and_a_host_that_accepts_it() {
    assert!(
        Session::from(
            "s".into(),
            "t".into(),
            &info(true, true),
            SessionFormat::Xlsx,
            0
        )
        .editable
    );
    assert!(
        !Session::from(
            "s".into(),
            "t".into(),
            &info(true, false),
            SessionFormat::Xlsx,
            0
        )
        .editable
    );
    assert!(
        !Session::from(
            "s".into(),
            "t".into(),
            &info(false, true),
            SessionFormat::Xlsx,
            0
        )
        .editable
    );
}

/// **The format comes from the host's filename, and an unrecognised one has no
/// format at all** (`WOPI-05`).
///
/// `None` is the load-bearing half. Defaulting to `.xlsx` is what let a session
/// open somebody's file and write a package back over it under its own name;
/// the caller refuses on `None` before it locks anything.
#[test]
fn a_filename_names_the_format_and_an_unknown_one_names_nothing() {
    assert_eq!(format_for("Q3.xlsx"), Some(SessionFormat::Xlsx));
    assert_eq!(
        format_for("Books.CSV"),
        Some(SessionFormat::Delimited(b','))
    );
    assert_eq!(
        format_for("Books.tsv"),
        Some(SessionFormat::Delimited(b'\t'))
    );
    // The last dot wins, both ways round.
    assert_eq!(
        format_for("report.2024.csv"),
        Some(SessionFormat::Delimited(b','))
    );
    assert_eq!(
        format_for("archive.csv.gz"),
        None,
        "a compressed csv is not a csv, and writing one back as text destroys it"
    );

    assert_eq!(format_for("Books.ODS"), Some(SessionFormat::Ods));

    for refused in ["Old.xls", "Flat.fods", "README", "", ".csvx", "trailing."] {
        assert_eq!(
            format_for(refused),
            None,
            "{refused:?} would have been opened and saved back as something else"
        );
    }
}

/// **A full registry hands the session back rather than dropping it.**
///
/// By the time a session is inserted its file is already locked on the host.
/// Dropping it on the floor leaves that lock held by something no request can
/// reach and no cleanup can find, so the file stays locked until WOPI's own
/// 30-minute expiry — for every user who arrives while the node is full.
#[test]
fn a_full_registry_returns_the_session_so_its_lock_can_be_released() {
    let sessions = Sessions::new(1, 60_000);
    let first = Session::from(
        "a".into(),
        "t".into(),
        &info(true, true),
        SessionFormat::Xlsx,
        0,
    );
    sessions.insert(first, 0).expect("room for one");

    let second = Session::from(
        "b".into(),
        "t2".into(),
        &info(true, true),
        SessionFormat::Xlsx,
        0,
    );
    let returned = sessions.insert(second, 0).expect_err("the node is full");
    assert_eq!(
        returned.src, "b",
        "the caller gets back what it must unlock"
    );
    assert_eq!(returned.token, "t2");
}

/// **An expired session is gone, and is handed back so its lock is released.**
#[test]
fn sessions_expire_and_are_collected() {
    let sessions = Sessions::new(10, 1_000);
    let id = sessions
        .insert(
            Session::from(
                "a".into(),
                "t".into(),
                &info(true, true),
                SessionFormat::Xlsx,
                0,
            ),
            0,
        )
        .expect("inserted");
    sessions.set_lock(&id, Some("lock-1".to_owned()), 0);

    assert!(sessions.get(&id, 999).is_some(), "still young");
    assert!(sessions.get(&id, 1_000).is_none(), "aged out");

    let collected = sessions.take_expired(1_000);
    assert_eq!(collected.len(), 1);
    assert_eq!(
        collected[0].lock.as_deref(),
        Some("lock-1"),
        "the lock comes back so it can be released"
    );
    assert_eq!(sessions.len(), 0);
}

/// **A lock is due a refresh on the clock, not on activity.**
///
/// WOPI locks expire after 30 minutes. A document left open and untouched is
/// exactly the one whose lock must survive.
#[test]
fn locks_come_due_on_a_timer() {
    let sessions = Sessions::new(10, 3_600_000);
    let id = sessions
        .insert(
            Session::from(
                "a".into(),
                "t".into(),
                &info(true, true),
                SessionFormat::Xlsx,
                0,
            ),
            0,
        )
        .expect("inserted");

    // Nothing to refresh before a lock is taken: refreshing a lock we do not
    // hold is a 409 against somebody else's.
    assert!(sessions.due_for_refresh(600_000, 600_000).is_empty());

    sessions.set_lock(&id, Some("lock-1".to_owned()), 0);
    assert!(sessions.due_for_refresh(599_999, 600_000).is_empty());
    assert_eq!(sessions.due_for_refresh(600_000, 600_000).len(), 1);

    // Refreshing resets the clock.
    sessions.set_lock(&id, Some("lock-1".to_owned()), 600_000);
    assert!(sessions.due_for_refresh(600_001, 600_000).is_empty());
}
