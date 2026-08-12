//! How a node is exposed: plain or TLS, direct or behind a reverse proxy, and
//! the same choices again for the port other nodes talk to.
//!
//! Every deployment answers these differently and none of the answers is a
//! default worth imposing. A single-container evaluation wants plain HTTP on
//! one port. A Kubernetes deployment terminates TLS at an ingress and wants
//! plain behind it — but must still learn the real client address, which only
//! the ingress knows. A regulated deployment wants TLS all the way to the
//! process, including between its own nodes.
//!
//! # The dangerous part is the forwarded headers, not the certificates
//!
//! `X-Forwarded-For` is a header, which means anyone who can reach the port can
//! write one. A server that believes it unconditionally has no idea who its
//! clients are: every rate limit, audit line and IP allow-list is then keyed on
//! a value the client chose. So the headers are believed **only when the
//! immediate peer is a configured proxy**, and the walk through the chain is
//! right-to-left rather than left-to-right — see [`ProxyTrust::client_ip`],
//! where that distinction is the whole difference between a spoofable value and
//! a trustworthy one.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

/// Where a certificate and its key live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsFiles {
    /// PEM certificate chain, leaf first.
    pub certificate: PathBuf,
    /// PEM private key.
    pub key: PathBuf,
}

/// One port this node listens on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The address to bind.
    pub bind: SocketAddr,
    /// TLS, or plain when absent.
    ///
    /// Absent is a legitimate production choice **behind a proxy that
    /// terminates TLS**, and a bad one anywhere else. The server cannot tell
    /// which situation it is in, so it does not pretend to: see
    /// [`Exposure::warnings`], which says so at startup rather than never.
    pub tls: Option<TlsFiles>,
    /// A CA whose certificates a *connecting* peer must present.
    ///
    /// Mutual TLS, and it belongs on the internal endpoint rather than the
    /// public one: the peers are a known, small, operator-controlled set, which
    /// is exactly the situation client certificates are good at and the
    /// situation a browser is not. On the public endpoint this would mean
    /// issuing a certificate to every user's browser.
    pub client_ca: Option<PathBuf>,
}

impl Endpoint {
    /// A plain listener on `bind`.
    #[must_use]
    pub fn plain(bind: SocketAddr) -> Self {
        Self {
            bind,
            tls: None,
            client_ca: None,
        }
    }

    /// A TLS listener on `bind`.
    #[must_use]
    pub fn secured(bind: SocketAddr, certificate: PathBuf, key: PathBuf) -> Self {
        Self {
            bind,
            tls: Some(TlsFiles { certificate, key }),
            client_ca: None,
        }
    }

    /// Also require a connecting peer to present a certificate from `client_ca`.
    #[must_use]
    pub fn requiring_client_certificate(mut self, client_ca: PathBuf) -> Self {
        self.client_ca = Some(client_ca);
        self
    }

    /// Whether a connecting peer must present a certificate.
    #[must_use]
    pub fn requires_client_certificate(&self) -> bool {
        self.client_ca.is_some()
    }

    /// Whether this endpoint terminates TLS itself.
    #[must_use]
    pub fn is_tls(&self) -> bool {
        self.tls.is_some()
    }
}

/// Which peers may speak for somebody else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProxyTrust {
    /// The addresses of proxies in front of this node.
    ///
    /// Empty means **trust nothing**, which is the right default: a server that
    /// believes `X-Forwarded-For` from anyone has no idea who its clients are,
    /// and every rate limit, audit line and allow-list downstream is then keyed
    /// on a value the client chose for itself.
    pub proxies: Vec<IpAddr>,
    /// Trust forwarded headers from any peer.
    ///
    /// For a deployment where the process is only reachable through a proxy —
    /// a private network, a sidecar, a container with no published port. It is
    /// a statement about the network, and it is wrong the moment that stops
    /// being true.
    pub trust_any_peer: bool,
}

impl ProxyTrust {
    /// Trust no forwarded headers.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Trust the named proxies.
    #[must_use]
    pub fn behind(proxies: Vec<IpAddr>) -> Self {
        Self {
            proxies,
            trust_any_peer: false,
        }
    }

    /// Whether the **immediate peer** may speak for somebody else.
    ///
    /// This is the question `trust_any_peer` answers: "is whatever connected to
    /// me a proxy?" It is a statement about the network the process sits on.
    fn trusts_peer(&self, peer: IpAddr) -> bool {
        self.trust_any_peer || self.proxies.contains(&self.normalise(peer))
    }

    /// Whether an address **inside the chain** is one of our own proxies.
    ///
    /// Deliberately *not* affected by `trust_any_peer`, which took a test to
    /// notice. Conflating the two makes the walk treat every entry in the chain
    /// as a trusted hop and skip straight past the client to the peer — so a
    /// deployment that said "I am only reachable through a proxy" got the
    /// proxy's own address as every client's, which is the exact value the walk
    /// exists to avoid returning.
    fn is_known_proxy(&self, address: IpAddr) -> bool {
        self.proxies.contains(&self.normalise(address))
    }

    /// An IPv4-mapped IPv6 address and its IPv4 form are the same host.
    ///
    /// A dual-stack listener reports `::ffff:10.0.0.7` for a peer an operator
    /// configured as `10.0.0.7`, and a proxy list that does not match is a
    /// proxy list that silently trusts nobody.
    fn normalise(&self, address: IpAddr) -> IpAddr {
        match address {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(address, IpAddr::V4),
            IpAddr::V4(_) => address,
        }
    }

    /// The client's real address, given the socket peer and any
    /// `X-Forwarded-For`.
    ///
    /// **Right to left, not left to right.** The header is a list that each hop
    /// appends to, so the rightmost entries were added by the proxies nearest
    /// this server — the ones whose honesty is a configuration decision — and
    /// the leftmost was written by whoever spoke first, which includes the
    /// client itself. Taking the leftmost is the common implementation and it
    /// is exactly backwards: a client that sends
    /// `X-Forwarded-For: 10.0.0.1` has then chosen its own address.
    ///
    /// So: skip entries contributed by proxies this node trusts, and the first
    /// one that is not a trusted proxy is the furthest hop this node has any
    /// reason to believe.
    #[must_use]
    pub fn client_ip(&self, peer: SocketAddr, forwarded_for: Option<&str>) -> IpAddr {
        let peer_ip = peer.ip();
        if !self.trusts_peer(peer_ip) {
            // Not a proxy, so it speaks only for itself whatever it claims.
            return peer_ip;
        }
        let Some(header) = forwarded_for else {
            return peer_ip;
        };
        for entry in header.rsplit(',') {
            let candidate = entry.trim().trim_matches('"');
            // An unparseable entry ends the walk. A chain this server cannot
            // read is one it should stop believing at, rather than skipping
            // past to something further away and less accountable.
            let Ok(address) = candidate.parse::<IpAddr>() else {
                return peer_ip;
            };
            if !self.is_known_proxy(address) {
                return address;
            }
        }
        // Every hop was a trusted proxy, so the nearest one is as far as this
        // goes.
        peer_ip
    }

    /// Whether the client's own connection was encrypted.
    ///
    /// `listener_is_tls` is what this process knows for certain;
    /// `forwarded_proto` is what a proxy says about the leg it terminated, and
    /// is believed on the same terms as the address.
    #[must_use]
    pub fn client_used_tls(
        &self,
        peer: SocketAddr,
        forwarded_proto: Option<&str>,
        listener_is_tls: bool,
    ) -> bool {
        if listener_is_tls {
            return true;
        }
        if !self.trusts_peer(peer.ip()) {
            return false;
        }
        forwarded_proto.is_some_and(|proto| {
            // A proxy may list a chain here too; the leftmost is the client's
            // own leg, which is the one being asked about.
            proto
                .split(',')
                .next()
                .is_some_and(|first| first.trim().eq_ignore_ascii_case("https"))
        })
    }
}

/// Who this node is, and how a peer reaches it.
///
/// # Two address spaces, and this is the internal one
///
/// A client reaches a node **through** whatever the operator put in front of
/// it — an ingress, a load balancer, a reverse proxy — and often the node does
/// not know that address at all, because the proxy owns it.
///
/// Relay and replication do not go that way. A node dials another **directly**,
/// on the cluster network, at the address it found in Redis. So this is an
/// internal address for an internal endpoint, and routing peer traffic back out
/// through the public proxy would be both slower and a way for cluster traffic
/// to arrive looking like a client.
///
/// Two consequences are enforced rather than described:
///
/// - It is checked against the **internal** endpoint's port, not the public
///   one. A node advertising its public address sends every peer through the
///   proxy.
/// - The internal endpoint never honours forwarded headers. A peer is not a
///   proxy, there is no hop between them to describe, and believing one there
///   would let anything that can reach the cluster port claim to be anything.
///
/// # Advertising is not binding
///
/// A node binds `0.0.0.0:8443` so it accepts connections on every interface,
/// and that is precisely the address no peer can dial. The same goes for a
/// container's `127.0.0.1`, and for a pod IP that is correct until the pod is
/// rescheduled. So the address peers are told is **separate configuration**,
/// not derived from the listener — deriving it is the mistake that makes a
/// cluster look configured and never form.
///
/// [`Self::problems`] refuses the shapes that cannot work rather than letting
/// them fail later as an unexplained absence of peers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeIdentity {
    /// A stable name for this node, unique in the cluster.
    ///
    /// Used as the discovery key and in the leader lease, so two nodes sharing
    /// one id are two nodes claiming to be the same leader. Give it something
    /// the orchestrator already guarantees is unique — a pod name, a task id —
    /// rather than a hostname that may repeat.
    pub id: String,
    /// The address **other nodes** should connect to, as `host:port`.
    ///
    /// On the cluster network, reaching the *internal* endpoint — not the public
    /// one, and not the proxy in front of it.
    ///
    /// A **string, not a `SocketAddr`**, and that is a correction rather than
    /// laxity. Requiring a literal IP is wrong for every deployment this targets:
    /// on a compose network or in Kubernetes a node is reached by service name,
    /// and its IP changes when it restarts — so an operator would have to
    /// configure an address that is wrong by the time it is used. It was found
    /// by the cluster compose refusing to start on `collab-a:8443`, which is
    /// exactly what such a deployment must supply.
    pub advertise: String,
}

impl NodeIdentity {
    /// The port peers are told to connect to, if the address names one.
    #[must_use]
    pub fn advertised_port(&self) -> Option<u16> {
        self.advertise.rsplit_once(':')?.1.parse().ok()
    }

    /// Why this identity could not be used to form a cluster, if it could not.
    ///
    /// Checked rather than assumed because every failure here presents the same
    /// way — peers that never appear, a leader that is never elected, documents
    /// that quietly run standalone on several nodes at once — and none of it
    /// points at the address that caused it.
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.id.trim().is_empty() {
            out.push("the node id is empty, and it is the key other nodes find this one by".into());
        }
        let (host, port) = match self.advertise.rsplit_once(':') {
            Some((host, port)) => (host, port),
            None => {
                out.push(format!(
                    "advertising {:?} has no port: peers need somewhere to connect, not just \
                     somewhere to look",
                    self.advertise
                ));
                return out;
            }
        };
        // Brackets are how an IPv6 literal carries a port; the address inside
        // is what the checks below are about. Missing this let `[::]:8443` —
        // "every interface", which is not somewhere to dial — pass as an
        // ordinary hostname.
        let host = host.trim_start_matches('[').trim_end_matches(']');
        // The checks that were here for an IP, kept for the cases a name cannot
        // rescue: these are wrong however they are spelled.
        if host.is_empty() || host == "0.0.0.0" || host == "::" {
            out.push(format!(
                "advertising {:?} tells peers to connect to \"any address\", which is not one: \
                 a bind address of 0.0.0.0 accepts from everywhere and can be dialled from \
                 nowhere",
                self.advertise
            ));
        }
        if host == "localhost" || host == "127.0.0.1" || host == "::1" {
            out.push(format!(
                "advertising {:?} tells every peer to connect to itself",
                self.advertise
            ));
        }
        if port.parse::<u16>().map_or(true, |p| p == 0) {
            out.push(format!(
                "advertising {:?} names a port nothing can listen on",
                self.advertise
            ));
        }
        out
    }
}

/// How this node is exposed, in full.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exposure {
    /// The port clients connect to.
    pub public: Endpoint,
    /// The port other nodes connect to, when a cluster is running.
    ///
    /// Separate from [`public`](Self::public) because the two are different
    /// security problems with different answers: the public port faces
    /// browsers over the internet, and this one faces a handful of known peers
    /// over a network the operator controls. A deployment may reasonably
    /// terminate TLS at an ingress for the first and require it end-to-end for
    /// the second, or the reverse.
    pub internal: Option<Endpoint>,
    /// Whose forwarded headers to believe **on the public endpoint**.
    ///
    /// It does not apply to [`internal`](Self::internal), and that is not an
    /// omission: a peer is not a proxy, there is no hop between two nodes for a
    /// header to describe, and honouring one there would let anything that
    /// reaches the cluster port claim to be anything.
    pub proxy: ProxyTrust,
    /// Who this node is to its peers. Absent in standalone, where there are
    /// none — which is why it is an `Option` rather than a value nobody uses.
    pub node: Option<NodeIdentity>,
}

impl Exposure {
    /// A single plain listener and no proxy trust — the evaluation shape.
    #[must_use]
    pub fn plain(bind: SocketAddr) -> Self {
        Self {
            public: Endpoint::plain(bind),
            internal: None,
            proxy: ProxyTrust::none(),
            node: None,
        }
    }

    /// What is worth saying out loud at startup.
    ///
    /// A configuration can be wrong in ways nothing will ever fail on: plain
    /// HTTP with no proxy in front of it works perfectly and carries every
    /// token in clear. These are the combinations that are *probably* a
    /// mistake, said once at startup rather than never — because the operator
    /// who needs to hear it is not reading this file.
    #[must_use]
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.public.is_tls() && self.proxy == ProxyTrust::none() {
            out.push(
                "the public endpoint is plain HTTP with no proxy configured in front of it: \
                 every token and every document travels in clear"
                    .to_owned(),
            );
        }
        if self.public.is_tls() && self.proxy.trust_any_peer {
            out.push(
                "the public endpoint terminates TLS itself and also trusts forwarded headers \
                 from any peer: a client can then choose the address it is logged as"
                    .to_owned(),
            );
        }
        if let Some(internal) = &self.internal {
            if !internal.is_tls() {
                out.push(
                    "the internal endpoint is plain: replication and relay traffic between \
                     nodes travels in clear, which is a decision about the network it sits on"
                        .to_owned(),
                );
            }
            if self.node.is_none() {
                out.push(
                    "an internal endpoint is configured but the node has no identity, so no \
                     peer can be told where to find it"
                        .to_owned(),
                );
            }
            if let Some(node) = &self.node {
                if let Some(advertised) = node.advertised_port()
                    && advertised != internal.bind.port()
                {
                    out.push(format!(
                        "this node advertises port {advertised} but its internal endpoint listens \
                         on {}: peers will dial a port nothing is serving them on",
                        internal.bind.port()
                    ));
                }
                if node.advertised_port() == Some(self.public.bind.port())
                    && self.public.bind.port() != internal.bind.port()
                {
                    out.push(
                        "this node advertises its public port to peers: relay and replication \
                         would go out through the proxy in front of it, which is slower and \
                         makes cluster traffic arrive looking like a client"
                            .to_owned(),
                    );
                }
                if internal.is_tls() && !internal.requires_client_certificate() {
                    out.push(
                        "the internal endpoint is encrypted but accepts any peer that can \
                         reach it: without a client CA, TLS here proves the traffic is \
                         private and not that the peer is one of yours"
                            .to_owned(),
                    );
                }
            }
            if internal.bind == self.public.bind {
                out.push(
                    "the internal and public endpoints share an address, so anything that can \
                     reach a client port can reach the cluster port"
                        .to_owned(),
                );
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
