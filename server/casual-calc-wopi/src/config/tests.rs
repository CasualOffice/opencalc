//! The allow-list is the only thing standing between an action URL and a
//! server-side request forgery, so most of these are attempts to get past it.

use super::*;

fn config(hosts: &[&str], allow_plain: bool) -> Config {
    Config {
        proof_key_path: None,
        bind: "127.0.0.1:0".into(),
        public_url: "https://calc.example".into(),
        internal_url: "http://wopi:8090".into(),
        collab_url: "/collab".into(),
        editor_url: "/editor/editor.html?mode=wopi".into(),
        secret: "0123456789abcdef".into(),
        audience: "opencalc-collab".into(),
        allowed_hosts: hosts.iter().map(|h| (*h).to_owned()).collect(),
        allow_plain,
        max_sessions: 10,
        session_ttl_ms: 1000,
        max_document_bytes: 1024,
        brand: crate::discovery::Brand::default(),
    }
}

/// **A host on the list is fetched; anything else is not.**
#[test]
fn only_listed_hosts_are_reachable() {
    let c = config(&["nc.example"], false);
    assert!(c.permits("https://nc.example/wopi/files/7").is_ok());
    assert!(c.permits("https://nc.example:443/wopi/files/7").is_ok());
    // Case is not a distinction in a hostname.
    assert!(c.permits("https://NC.Example/wopi/files/7").is_ok());

    let why = c
        .permits("https://elsewhere.example/wopi/files/7")
        .expect_err("not listed");
    assert_eq!(
        why,
        "elsewhere.example is not in OPENCALC_WOPI_ALLOWED_HOSTS"
    );

    // **A WOPISrc may be all authority and query, with no path at all.**
    // SharePoint's are. An authority taken by splitting on `/` swallows the
    // query with it and rejects a host that is on the list — which fails safe
    // and also fails to work, and presents as "the allow-list is ignored".
    assert!(
        c.permits("https://nc.example?a=b").is_ok(),
        "query, no path"
    );
    assert!(c.permits("https://nc.example").is_ok(), "bare authority");
}

/// **The authority ends at the first `/`, `?` or `#`.**
///
/// `https://evil.example?x=/nc.example` has the allowed host in it, and a check
/// that splits on `/` alone reads it as the authority. The request would go to
/// `evil.example`, carrying an access token.
#[test]
fn a_query_string_cannot_impersonate_the_authority() {
    let c = config(&["nc.example"], false);
    for disguise in [
        "https://evil.example?x=/nc.example",
        "https://evil.example#/nc.example",
        "https://evil.example/nc.example/wopi",
    ] {
        let why = c.permits(disguise).expect_err(disguise);
        // Exactly the authority, not merely a string containing it: a parser
        // that reports `evil.example?x=` is also rejecting, and would satisfy a
        // `contains` while being wrong about what it looked at.
        assert_eq!(
            why, "evil.example is not in OPENCALC_WOPI_ALLOWED_HOSTS",
            "{disguise}"
        );
    }
}

/// **Credentials in a URL do not move the authority.**
///
/// `https://nc.example@evil.example/` reads to a person as the allowed host and
/// resolves, in every URL parser, to `evil.example`.
#[test]
fn userinfo_cannot_impersonate_the_authority() {
    let c = config(&["nc.example"], false);
    let why = c
        .permits("https://nc.example@evil.example/wopi/files/7")
        .expect_err("userinfo trick");
    assert_eq!(why, "evil.example is not in OPENCALC_WOPI_ALLOWED_HOSTS");
}

/// **Plain `http` is refused unless a deployment opts in.**
///
/// The access token travels in the query string. Over `http` it is on the wire
/// in clear, and it is a credential for somebody else's file store.
#[test]
fn plain_http_is_off_by_default() {
    let strict = config(&["nc.example"], false);
    assert!(strict.permits("http://nc.example/wopi/files/7").is_err());

    let lax = config(&["nc.example"], true);
    assert!(lax.permits("http://nc.example/wopi/files/7").is_ok());
    // Opting into plain HTTP does not opt out of the allow-list.
    assert!(lax.permits("http://evil.example/wopi/files/7").is_err());
}

/// **Anything that is not an absolute http(s) URL is refused.**
///
/// `file:///etc/passwd` is the reason this is a prefix check and not a
/// "does it contain the host" check.
#[test]
fn only_absolute_http_urls_are_accepted() {
    let c = config(&["nc.example"], true);
    for bad in [
        "file:///etc/passwd",
        "gopher://nc.example/",
        "//nc.example/wopi",
        "/wopi/files/7",
        "nc.example/wopi",
        "",
    ] {
        assert!(c.permits(bad).is_err(), "{bad} was permitted");
    }
}

/// **An empty allow-list stops the process rather than defaulting open.**
///
/// The failure of a defaulted-open list is silent and remote: nothing is wrong
/// until somebody sends a link, and by then the request has already been made
/// from inside the perimeter.
#[test]
fn an_empty_allow_list_is_a_startup_failure() {
    // `from_env` reads process-wide state, so this asserts on the message the
    // operator gets rather than racing other tests over the environment.
    let c = config(&[], false);
    assert!(
        c.permits("https://nc.example/wopi/files/7").is_err(),
        "nothing is reachable with an empty list"
    );
}
