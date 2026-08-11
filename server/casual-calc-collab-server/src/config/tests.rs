//! Exposure tests, concentrated on the one thing here that is a security
//! boundary: whose `X-Forwarded-For` is believed.

use super::*;

fn peer(ip: &str) -> SocketAddr {
    SocketAddr::new(ip.parse().unwrap(), 40_000)
}

fn node() -> NodeIdentity {
    NodeIdentity {
        id: "node-a".into(),
        advertise: peer("10.0.0.1"),
    }
}

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

// --- Who is believed -------------------------------------------------------

#[test]
fn a_client_cannot_choose_its_own_address() {
    // The attack this exists to stop. Anyone who can reach the port can send
    // the header; a server that believes it has no idea who its clients are,
    // and every rate limit and audit line downstream is keyed on a value the
    // client picked.
    let trust = ProxyTrust::none();
    assert_eq!(
        trust.client_ip(peer("203.0.113.9"), Some("10.0.0.1")),
        ip("203.0.113.9"),
        "an untrusted peer speaks only for itself"
    );
}

#[test]
fn a_configured_proxy_is_believed() {
    let trust = ProxyTrust::behind(vec![ip("10.0.0.7")]);
    assert_eq!(
        trust.client_ip(peer("10.0.0.7"), Some("203.0.113.9")),
        ip("203.0.113.9")
    );
}

#[test]
fn the_chain_is_walked_from_the_right_and_not_from_the_left() {
    // The distinction that decides whether the value is trustworthy. Each hop
    // *appends*, so the rightmost entries came from the proxies nearest this
    // server — the ones whose honesty is a configuration decision — and the
    // leftmost was written by whoever spoke first, which includes the client.
    //
    // Here a client sent a forged entry before the real chain was appended.
    // Taking the leftmost, which is the common implementation, would return
    // the forgery.
    let trust = ProxyTrust::behind(vec![ip("10.0.0.7"), ip("10.0.0.8")]);
    assert_eq!(
        trust.client_ip(peer("10.0.0.7"), Some("192.0.2.1, 203.0.113.9, 10.0.0.8"),),
        ip("203.0.113.9"),
        "the furthest hop this server has reason to believe, not the first \
         thing the client wrote"
    );
}

#[test]
fn trusted_proxies_in_the_chain_are_skipped_and_untrusted_ones_stop_it() {
    let trust = ProxyTrust::behind(vec![ip("10.0.0.7"), ip("10.0.0.8")]);
    // Every hop trusted: fall back to the peer, because there is no client
    // entry left to believe.
    assert_eq!(
        trust.client_ip(peer("10.0.0.7"), Some("10.0.0.8")),
        ip("10.0.0.7")
    );
    // One untrusted hop among them ends the walk there.
    assert_eq!(
        trust.client_ip(peer("10.0.0.7"), Some("198.51.100.4, 10.0.0.8")),
        ip("198.51.100.4")
    );
}

#[test]
fn an_unreadable_chain_is_not_walked_past() {
    // A chain this server cannot parse is one it should stop believing at,
    // rather than skipping to something further away and less accountable.
    let trust = ProxyTrust::behind(vec![ip("10.0.0.7")]);
    assert_eq!(
        trust.client_ip(peer("10.0.0.7"), Some("203.0.113.9, not-an-address")),
        ip("10.0.0.7")
    );
    assert_eq!(trust.client_ip(peer("10.0.0.7"), Some("")), ip("10.0.0.7"));
    assert_eq!(
        trust.client_ip(peer("10.0.0.7"), Some(",,,")),
        ip("10.0.0.7")
    );
}

#[test]
fn a_missing_header_leaves_the_peer_as_the_client() {
    let trust = ProxyTrust::behind(vec![ip("10.0.0.7")]);
    assert_eq!(trust.client_ip(peer("10.0.0.7"), None), ip("10.0.0.7"));
}

#[test]
fn trusting_any_peer_is_a_statement_about_the_network() {
    // For a process only reachable through a proxy. It is right until that
    // stops being true, which is why it is not a default.
    let trust = ProxyTrust {
        proxies: Vec::new(),
        trust_any_peer: true,
    };
    assert_eq!(
        trust.client_ip(peer("172.16.0.3"), Some("203.0.113.9")),
        ip("203.0.113.9")
    );
}

#[test]
fn ipv6_entries_are_handled_like_any_other() {
    let trust = ProxyTrust::behind(vec![ip("::1")]);
    assert_eq!(
        trust.client_ip(peer("::1"), Some("2001:db8::5")),
        ip("2001:db8::5")
    );
}

// --- Was the client's own leg encrypted? -----------------------------------

#[test]
fn a_tls_listener_needs_nobody_to_vouch_for_it() {
    let trust = ProxyTrust::none();
    assert!(trust.client_used_tls(peer("203.0.113.9"), None, true));
    assert!(
        trust.client_used_tls(peer("203.0.113.9"), Some("http"), true),
        "what this process observed beats what a header claims"
    );
}

#[test]
fn a_forwarded_scheme_is_believed_on_the_same_terms_as_an_address() {
    let trusted = ProxyTrust::behind(vec![ip("10.0.0.7")]);
    assert!(trusted.client_used_tls(peer("10.0.0.7"), Some("https"), false));
    assert!(!trusted.client_used_tls(peer("10.0.0.7"), Some("http"), false));

    // The same header from somebody who is not a proxy proves nothing.
    assert!(!ProxyTrust::none().client_used_tls(peer("203.0.113.9"), Some("https"), false));
}

#[test]
fn the_clients_own_leg_is_the_leftmost_scheme() {
    // The mirror image of the address rule, and not a contradiction of it: a
    // chain of schemes describes the same hops, and the question here is about
    // the *client's* leg, which is the first one.
    let trust = ProxyTrust::behind(vec![ip("10.0.0.7")]);
    assert!(trust.client_used_tls(peer("10.0.0.7"), Some("https, http"), false));
    assert!(!trust.client_used_tls(peer("10.0.0.7"), Some("http, https"), false));
    assert!(
        trust.client_used_tls(peer("10.0.0.7"), Some("HTTPS"), false),
        "case is not part of the value"
    );
}

// --- Endpoints -------------------------------------------------------------

#[test]
fn an_endpoint_is_plain_or_secured_and_says_which() {
    let plain = Endpoint::plain(peer("127.0.0.1"));
    assert!(!plain.is_tls());
    let secured = Endpoint::secured(peer("127.0.0.1"), "cert.pem".into(), "key.pem".into());
    assert!(secured.is_tls());
}

// --- What is said at startup ------------------------------------------------

#[test]
fn plain_with_no_proxy_is_called_out() {
    // It works perfectly and carries every token in clear, which is exactly why
    // nothing will ever fail to tell the operator.
    let warnings = Exposure::plain(peer("0.0.0.0")).warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("in clear"));
}

#[test]
fn plain_behind_a_configured_proxy_is_not_called_out() {
    // The ordinary production shape: TLS terminates at an ingress and this
    // process listens plain behind it.
    let exposure = Exposure {
        public: Endpoint::plain(peer("0.0.0.0")),
        internal: None,
        proxy: ProxyTrust::behind(vec![ip("10.0.0.7")]),
        node: None,
    };
    assert!(exposure.warnings().is_empty(), "{:?}", exposure.warnings());
}

#[test]
fn terminating_tls_and_trusting_everybody_is_called_out() {
    // Contradictory: the connection is direct, so there is no proxy whose
    // headers could be worth anything — and trusting them lets a client choose
    // the address it is logged as.
    let exposure = Exposure {
        public: Endpoint::secured(peer("0.0.0.0"), "c.pem".into(), "k.pem".into()),
        internal: None,
        proxy: ProxyTrust {
            proxies: Vec::new(),
            trust_any_peer: true,
        },
        node: None,
    };
    let warnings = exposure.warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("choose the address"));
}

#[test]
fn a_plain_internal_endpoint_is_called_out_as_a_decision_about_the_network() {
    let exposure = Exposure {
        public: Endpoint::secured(peer("0.0.0.0"), "c.pem".into(), "k.pem".into()),
        internal: Some(Endpoint::plain(peer("10.0.0.1"))),
        proxy: ProxyTrust::none(),
        node: Some(node()),
    };
    let warnings = exposure.warnings();
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("between \nnodes") || w.contains("nodes")),
        "{warnings:?}"
    );
}

#[test]
fn sharing_one_address_between_the_client_and_cluster_ports_is_called_out() {
    let both = peer("0.0.0.0");
    let exposure = Exposure {
        public: Endpoint::secured(both, "c.pem".into(), "k.pem".into()),
        internal: Some(Endpoint::secured(both, "c.pem".into(), "k.pem".into())),
        proxy: ProxyTrust::none(),
        node: Some(node()),
    };
    assert!(
        exposure
            .warnings()
            .iter()
            .any(|w| w.contains("share an address")),
        "{:?}",
        exposure.warnings()
    );
}

#[test]
fn a_fully_secured_deployment_has_nothing_to_say() {
    let exposure = Exposure {
        public: Endpoint::secured(peer("0.0.0.0"), "c.pem".into(), "k.pem".into()),
        internal: Some(Endpoint::secured(
            peer("10.0.0.1"),
            "c.pem".into(),
            "k.pem".into(),
        )),
        proxy: ProxyTrust::none(),
        node: Some(node()),
    };
    assert!(exposure.warnings().is_empty(), "{:?}", exposure.warnings());
}

// --- Node identity ----------------------------------------------------------

#[test]
fn a_reachable_identity_has_no_problems() {
    assert!(node().problems().is_empty());
}

#[test]
fn advertising_an_address_no_peer_can_dial_is_refused() {
    // The mistake that makes a cluster look configured and never form. A bind
    // address of 0.0.0.0 accepts from everywhere and can be dialled from
    // nowhere, and every symptom of getting this wrong — no peers, no leader,
    // several nodes quietly running the same document standalone — points
    // somewhere else.
    for bad in ["0.0.0.0:8443", "[::]:8443"] {
        let identity = NodeIdentity {
            id: "node-a".into(),
            advertise: bad.parse().unwrap(),
        };
        assert!(
            identity
                .problems()
                .iter()
                .any(|p| p.contains("any address")),
            "should refuse {bad}: {:?}",
            identity.problems()
        );
    }
}

#[test]
fn advertising_loopback_tells_every_peer_to_connect_to_itself() {
    let identity = NodeIdentity {
        id: "node-a".into(),
        advertise: "127.0.0.1:8443".parse().unwrap(),
    };
    assert!(
        identity.problems().iter().any(|p| p.contains("to itself")),
        "{:?}",
        identity.problems()
    );
}

#[test]
fn an_empty_id_or_a_zero_port_is_refused() {
    let identity = NodeIdentity {
        id: "  ".into(),
        advertise: "10.0.0.1:0".parse().unwrap(),
    };
    let problems = identity.problems();
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("node id is empty")));
    assert!(problems.iter().any(|p| p.contains("port 0")));
}

#[test]
fn a_cluster_endpoint_without_an_identity_is_called_out() {
    // Listening for peers while telling none of them where to find you.
    let exposure = Exposure {
        public: Endpoint::secured(peer("0.0.0.0"), "c.pem".into(), "k.pem".into()),
        internal: Some(Endpoint::secured(
            peer("10.0.0.1"),
            "c.pem".into(),
            "k.pem".into(),
        )),
        proxy: ProxyTrust::none(),
        node: None,
    };
    assert!(
        exposure
            .warnings()
            .iter()
            .any(|w| w.contains("no peer can be told")),
        "{:?}",
        exposure.warnings()
    );
}
