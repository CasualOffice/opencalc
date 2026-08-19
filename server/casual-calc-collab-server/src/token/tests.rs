//! Token tests: what is admitted, what is refused, and what a permission
//! actually stops.

use casual_calc_model::{Cell, CellRef, CellValue};
use casual_calc_transaction::{Operation, SheetFields, SheetMetadata};

use super::*;

fn claims() -> Claims {
    Claims {
        iss: "https://host.example".into(),
        aud: "opencalc-collab".into(),
        exp: 2_000,
        iat: Some(1_000),
        nbf: None,
        jti: None,
        user: User {
            id: "u-17".into(),
            name: "Ada".into(),
            email: None,
            avatar_url: None,
            group: None,
            color: None,
        },
        document: Document {
            key: "doc-1-rev-9".into(),
            id: "file-1".into(),
            title: "Budget.xlsx".into(),
            version: Some("9".into()),
            owner_id: Some("u-1".into()),
            url: "https://host.example/files/1".into(),
        },
        permissions: Permissions {
            access: Access::Edit,
            download: true,
            print: true,
            copy: true,
        },
        owner: false,
        callback: Some(Callback::Url {
            url: "https://host.example/callback".into(),
        }),
    }
}

fn policy() -> TokenPolicy {
    TokenPolicy {
        audience: "opencalc-collab".into(),
        leeway_secs: 30,
        allowed_hosts: BTreeSet::new(),
        require_https: true,
    }
}

/// A cell edit, which only an editor may send.
fn cell_edit() -> Operation {
    Operation::SetCell {
        sheet: 0,
        at: CellRef::new(0, 0),
        cell: Some(Cell::value(CellValue::Number(1.0))),
    }
}

/// A comment change, which a commenter may send.
fn comment_change() -> Operation {
    Operation::SetSheetMetadata {
        sheet: 0,
        data: Box::new(SheetMetadata::default()),
        changed: SheetFields::COMMENTS,
        restore: Default::default(),
    }
}

// --- Validation ------------------------------------------------------------

#[test]
fn a_well_formed_token_for_this_document_is_admitted() {
    assert_eq!(claims().validate("doc-1-rev-9", &policy(), 1_500), Ok(()));
}

#[test]
fn an_expired_token_is_refused_and_leeway_is_finite() {
    let c = claims();
    // Inside the skew allowance: the host's clock and ours differ, and a login
    // loop nobody can diagnose is the cost of being strict here.
    assert_eq!(c.validate("doc-1-rev-9", &policy(), 2_020), Ok(()));
    assert_eq!(
        c.validate("doc-1-rev-9", &policy(), 2_100),
        Err(TokenError::Expired)
    );
}

#[test]
fn a_token_that_is_not_valid_yet_is_refused() {
    let mut c = claims();
    c.nbf = Some(1_800);
    assert_eq!(
        c.validate("doc-1-rev-9", &policy(), 1_500),
        Err(TokenError::NotYetValid)
    );
    assert_eq!(
        c.validate("doc-1-rev-9", &policy(), 1_790),
        Ok(()),
        "leeway"
    );
}

#[test]
fn a_token_minted_for_another_service_cannot_be_replayed_here() {
    let mut c = claims();
    c.aud = "some-other-service".into();
    assert_eq!(
        c.validate("doc-1-rev-9", &policy(), 1_500),
        Err(TokenError::WrongAudience)
    );
}

#[test]
fn a_valid_token_for_one_document_does_not_admit_its_bearer_to_another() {
    // The check that matters most. A signature proves the host issued the
    // token, not that the host issued it *for this document* — without this,
    // anyone with a token for a file they may read could join any file on the
    // server.
    assert_eq!(
        claims().validate("some-other-document", &policy(), 1_500),
        Err(TokenError::WrongDocument)
    );
}

#[test]
fn a_token_missing_an_identity_is_refused() {
    let mut c = claims();
    c.user.id = String::new();
    assert_eq!(
        c.validate("doc-1-rev-9", &policy(), 1_500),
        Err(TokenError::Incomplete("a user id"))
    );
}

#[test]
fn what_the_client_is_told_is_less_than_what_the_log_records() {
    // Which of expired, wrong-audience or wrong-document it was is useful to an
    // operator and useful to an attacker. The client gets one answer.
    for e in [
        TokenError::Expired,
        TokenError::WrongAudience,
        TokenError::WrongDocument,
        TokenError::NotYetValid,
    ] {
        assert_eq!(e.refusal(), Refusal::NotAuthorised);
        assert!(!e.to_string().is_empty(), "and the operator gets detail");
    }
}

// --- URL policy ------------------------------------------------------------

#[test]
fn a_url_outside_the_allow_list_is_refused() {
    // A token names an address this server will connect to, which makes a
    // leaked one a request-forgery primitive pointed at whatever the server can
    // reach — including addresses inside the deployment.
    let mut p = policy();
    p.allowed_hosts.insert("host.example".into());

    assert_eq!(claims().validate("doc-1-rev-9", &p, 1_500), Ok(()));

    let mut c = claims();
    c.callback = Some(Callback::Url {
        url: "https://attacker.example/collect".into(),
    });
    assert_eq!(
        c.validate("doc-1-rev-9", &p, 1_500),
        Err(TokenError::ForbiddenUrl {
            url: "https://attacker.example/collect".into(),
            hint: None,
        })
    );
}

#[test]
fn the_allow_list_is_not_fooled_by_the_shape_of_a_url() {
    // Userinfo before the host, and a port after it, are the two ways a naive
    // "does it contain the allowed host" check gets this wrong.
    let mut p = policy();
    p.allowed_hosts.insert("host.example".into());

    for url in [
        "https://host.example:8443/files/1",
        "https://user:pw@host.example/files/1",
        "https://host.example/files/1?x=1#y",
    ] {
        assert!(check_url(url, &p).is_ok(), "should allow {url}");
    }
    for url in [
        // The allowed host as userinfo, with the real host elsewhere.
        "https://host.example@attacker.example/collect",
        // As a subdomain of the attacker's.
        "https://host.example.attacker.example/collect",
        // As a path.
        "https://attacker.example/host.example",
    ] {
        assert!(check_url(url, &p).is_err(), "should refuse {url}");
    }
}

#[test]
fn plain_http_is_refused_unless_it_was_asked_for() {
    let mut c = claims();
    c.callback = Some(Callback::Url {
        url: "http://host.example/callback".into(),
    });
    assert!(matches!(
        c.validate("doc-1-rev-9", &policy(), 1_500),
        Err(TokenError::ForbiddenUrl { .. })
    ));

    let mut p = policy();
    p.require_https = false;
    assert_eq!(c.validate("doc-1-rev-9", &p, 1_500), Ok(()));
}

/// The one misconfiguration the allow-list cannot express, said out loud.
///
/// `check_url` compares the host with the port stripped, so a natural-looking
/// `127.0.0.1:8080` never matches anything. The refusal used to name only the
/// URL, which reads as "your token is wrong" when the configuration is wrong —
/// and the operator is looking at an allow-list that appears to contain exactly
/// the host being refused.
#[test]
fn an_allowed_host_that_names_a_port_says_that_is_why_it_matched_nothing() {
    let mut p = policy();
    p.allowed_hosts.insert("127.0.0.1:8080".into());

    let refusal = check_url("https://127.0.0.1:8080/files/1", &p)
        .expect_err("the port is stripped, so this cannot match")
        .to_string();
    assert!(
        refusal.contains("port"),
        "the refusal has to name the reason, not only the url: {refusal}"
    );
    assert!(
        refusal.contains("127.0.0.1:8080"),
        "and the entry that can never match: {refusal}"
    );
}

/// The other half, which is what keeps the hint honest.
///
/// A hint printed on every refusal is not a diagnosis, it is noise — and it
/// would send an operator whose allow-list is fine to change it.
#[test]
fn an_ordinary_refusal_does_not_blame_a_port() {
    let mut p = policy();
    p.allowed_hosts.insert("host.example".into());
    // An IPv6 literal is nothing but colons and none of them is a port.
    p.allowed_hosts.insert("::1".into());
    p.allowed_hosts.insert("2001:db8::1".into());

    let refusal = check_url("https://attacker.example/collect", &p)
        .expect_err("not on the list")
        .to_string();
    assert!(
        !refusal.contains("port"),
        "nothing here names a port; the hint fired on an allow-list that is fine: {refusal}"
    );
}

/// **SEC-018. An IPv6 literal reaches the allow-list intact.**
///
/// The port used to be stripped with `rsplit_once(':')`, which for `[::1]`
/// splits at the last colon *of the address* — leaving `[:`, which trimmed to
/// `":"`. So `OPENCALC_ALLOWED_HOSTS=::1` matched nothing at all, and a
/// deployment reaching its host over IPv6 could not be configured to work.
/// It failed closed, so it is a dead allow-list entry rather than an open door
/// — the same shape as the `host:port` trap above, and just as invisible from
/// both ends: the entry looks right and the URL looks right.
#[test]
fn a_bracketed_ipv6_literal_matches_the_allowed_host_that_names_it() {
    let mut p = policy();
    p.allowed_hosts.insert("::1".into());

    for url in [
        "https://[::1]/files/1",
        "https://[::1]:8443/files/1",
        "https://user:pw@[::1]/files/1",
        "https://[::1]/files/1?x=1#y",
        // The same address, spelled the long way. An entry that *is* the host
        // being refused, in another spelling, is the defect again.
        "https://[0:0:0:0:0:0:0:1]/files/1",
    ] {
        assert_eq!(check_url(url, &p), Ok(()), "should allow {url}");
    }

    // And the operator may write the brackets a URL puts around it, because
    // that is what they will have copied from the URL.
    let mut bracketed = policy();
    bracketed.allowed_hosts.insert("[2001:db8::1]".into());
    assert_eq!(
        check_url("https://[2001:db8::1]:8443/files/1", &bracketed),
        Ok(())
    );

    // The allow-list still bites: another address is another host.
    assert!(check_url("https://[2001:db8::2]/files/1", &p).is_err());
    assert!(check_url("https://[::2]/files/1", &p).is_err());
}

/// The adversarial half. This is a URL from a signed token, and the signature
/// says the host issued it — not that it is well-formed, and not that whoever
/// obtained it means well. Every shape here must refuse, because each is a way
/// of reading one origin and resolving to another.
#[test]
fn an_authority_the_parser_cannot_read_is_refused_rather_than_guessed_at() {
    let mut p = policy();
    p.allowed_hosts.insert("::1".into());
    p.allowed_hosts.insert("host.example".into());

    for url in [
        // Unterminated, so where the host ends is a guess.
        "https://[::1/files/1",
        // Anything at all after the brackets that is not a port.
        "https://[::1]x/files/1",
        "https://[::1]:8443x/files/1",
        "https://[::1]:-1/files/1",
        // The allowed literal as *userinfo*, with the real host after the `@`.
        "https://[::1]@attacker.example/collect",
        // The allowed literal as the start of a longer name.
        "https://[::1].attacker.example/collect",
        // Brackets holding something that is not an address.
        "https://[not-an-address]/files/1",
        "https://[host.example]/files/1",
        "https://[]/files/1",
        // An IPv6 literal that forgot its brackets is not an authority.
        "https://::1/files/1",
        // A host that is not on the list, with and without a port.
        "https://attacker.example/collect",
        "https://attacker.example:8443/collect",
    ] {
        assert!(check_url(url, &p).is_err(), "should refuse {url}");
    }
}

/// The hint, for the bracketed spelling of the same dead entry.
///
/// `[::1]:8443` names a port, so the comparison can never match it — exactly
/// like `127.0.0.1:8080`, and the operator deserves the same sentence.
#[test]
fn an_allowed_host_that_brackets_an_address_and_names_a_port_is_diagnosed_too() {
    let mut p = policy();
    p.allowed_hosts.insert("[::1]:8443".into());

    let refusal = check_url("https://[::1]:8443/files/1", &p)
        .expect_err("the port is stripped, so this cannot match")
        .to_string();
    assert!(refusal.contains("port"), "{refusal}");
    assert!(refusal.contains("[::1]:8443"), "{refusal}");
}

#[test]
fn a_url_with_no_scheme_or_no_host_is_refused() {
    for url in ["", "host.example/files", "https://", "file:///etc/passwd"] {
        assert!(check_url(url, &policy()).is_err(), "should refuse {url:?}");
    }
}

// --- Permissions are enforced, not carried ---------------------------------

#[test]
fn a_viewer_may_send_nothing() {
    assert!(!Access::View.permits(&cell_edit()));
    assert!(!Access::View.permits(&comment_change()));
}

#[test]
fn a_commenter_may_comment_and_may_not_edit_a_cell() {
    // The reason `Comment` is a mode and not a label. Hiding a toolbar is a
    // suggestion; refusing the operation is the permission.
    assert!(Access::Comment.permits(&comment_change()));
    assert!(!Access::Comment.permits(&cell_edit()));
}

#[test]
fn a_commenter_may_not_smuggle_an_edit_through_a_metadata_bundle() {
    // A sheet-metadata operation carrying anything besides comments is not a
    // comment operation, however it is spelled.
    let sneaky = Operation::SetSheetMetadata {
        sheet: 0,
        data: Box::new(SheetMetadata::default()),
        changed: SheetFields::COMMENTS.union(SheetFields::VIEW),
        restore: Default::default(),
    };
    assert!(!Access::Comment.permits(&sneaky));
}

#[test]
fn a_commenter_may_not_smuggle_an_edit_inside_a_batch() {
    // A batch is exactly its members, so one bad member spoils it.
    assert!(Access::Comment.permits(&Operation::Batch(vec![comment_change(), comment_change()])));
    assert!(!Access::Comment.permits(&Operation::Batch(vec![comment_change(), cell_edit()])));
    assert!(
        !Access::Comment.permits(&Operation::Batch(vec![])),
        "an empty batch is not a comment"
    );
}

#[test]
fn an_editor_may_send_anything() {
    assert!(Access::Edit.permits(&cell_edit()));
    assert!(Access::Edit.permits(&comment_change()));
}

#[test]
fn access_levels_are_ordered_so_at_least_this_much_is_expressible() {
    assert!(Access::Edit > Access::Comment);
    assert!(Access::Comment > Access::View);
}

#[test]
fn the_default_permission_is_the_least_one() {
    // A token that forgets to say must not thereby grant editing.
    assert_eq!(Permissions::default().access, Access::View);
}

// --- Serialization ---------------------------------------------------------

#[test]
fn a_token_round_trips_through_json_as_a_host_would_sign_it() {
    let json = serde_json::to_string(&claims()).unwrap();
    assert_eq!(serde_json::from_str::<Claims>(&json).unwrap(), claims());

    // The registered claims keep JWT's own spelling, so an integrator's signing
    // code needs no translation table.
    for key in ["\"iss\"", "\"aud\"", "\"exp\"", "\"iat\""] {
        assert!(json.contains(key), "{key} missing from {json}");
    }
}

#[test]
fn the_optional_fields_may_all_be_absent() {
    // An integrator that does not want to hand personal data to an editing
    // service should not have to, and one without a callback is running a
    // preview rather than making a mistake.
    let json = r#"{
        "iss": "https://host.example",
        "aud": "opencalc-collab",
        "exp": 2000,
        "user": { "id": "u-17", "name": "Ada" },
        "document": {
            "key": "doc-1-rev-9",
            "id": "file-1",
            "title": "Budget.xlsx",
            "url": "https://host.example/files/1"
        }
    }"#;
    let parsed: Claims = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.permissions.access, Access::View, "the safe default");
    assert!(parsed.callback.is_none(), "no callback means no save");
    assert_eq!(parsed.validate("doc-1-rev-9", &policy(), 1_500), Ok(()));
}

#[test]
fn the_two_callback_shapes_are_told_apart_by_their_tag() {
    let wopi: Callback = serde_json::from_str(
        r#"{"kind":"wopi","src":"https://h.example/wopi/files/1","token":"t","token_expiry_ms":123}"#,
    )
    .unwrap();
    assert_eq!(wopi.endpoint(), "https://h.example/wopi/files/1");

    let url: Callback =
        serde_json::from_str(r#"{"kind":"url","url":"https://h.example/cb"}"#).unwrap();
    assert_eq!(url.endpoint(), "https://h.example/cb");
}

#[test]
fn access_serializes_as_the_word_an_integrator_would_write() {
    assert_eq!(serde_json::to_string(&Access::Edit).unwrap(), "\"edit\"");
    assert_eq!(
        serde_json::to_string(&Access::Comment).unwrap(),
        "\"comment\""
    );
    assert_eq!(serde_json::to_string(&Access::View).unwrap(), "\"view\"");
}
