//! The runnable collaboration server.
//!
//! Everything below this file is a library: state machines, a transport, a
//! verifier, a cluster. None of it could be *run*, which is a strange thing for
//! a server to be — the audit that swept this crate did not notice, because it
//! read what was there rather than trying to start it.
//!
//! # Configuration is environment, and it is checked at startup
//!
//! Twelve-factor, because the deployments this is for hand a container an
//! environment and nothing else. Every value is read once, here, and the ones
//! that cannot work together are refused or reported **before** the listener
//! opens — a node that starts and is subtly wrong costs more than one that
//! refuses to start and says why.

use std::net::SocketAddr;
use std::sync::Arc;

use casual_calc_collab_server::cluster::redis::Redis;
use casual_calc_collab_server::config::{Endpoint, Exposure, NodeIdentity, ProxyTrust};
use casual_calc_collab_server::http::{HttpConfig, HttpTransport};
use casual_calc_collab_server::lifecycle::SavePolicy;
use casual_calc_collab_server::net::{Limits, Membership, ServiceConfig, serve};
use casual_calc_collab_server::token::TokenPolicy;
use casual_calc_collab_server::verify::{KeySet, Signing, Verifier};
use casual_calc_transaction::session::SnapshotPolicy;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // A health check the image can run without a shell or an HTTP client in it.
    // The runtime layer is `debian:bookworm-slim` plus a CA bundle: no curl, no
    // wget, and adding one to answer a question the binary can answer itself is
    // weight in every deployment for the benefit of the orchestrator.
    if std::env::args().any(|a| a == "--healthcheck") {
        return match healthy().await {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(why) => {
                tracing::error!("{why}");
                std::process::ExitCode::FAILURE
            }
        };
    }

    match start().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(why) => {
            // Not a panic. A configuration problem is not a bug in this
            // program, and a backtrace would bury the one line that matters.
            tracing::error!("{why}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Ask the running server whether it is serving.
///
/// Reads the same `OPENCALC_BIND` the server did, so the check cannot drift from
/// the thing it checks by being told a port twice.
///
/// Over plain HTTP it fetches `/healthz`, which proves the whole request path
/// works and not merely that something is holding the port. With TLS configured
/// it settles for a TCP connection: verifying the certificate would need the
/// hostname an operator's clients use rather than the loopback address this
/// connects on, and disabling verification to get around that would make the
/// check pass against anything at all.
async fn healthy() -> Result<(), String> {
    let bind = std::env::var("OPENCALC_BIND").unwrap_or_else(|_| "0.0.0.0:8443".to_owned());
    let addr: SocketAddr = bind
        .parse()
        .map_err(|e| format!("OPENCALC_BIND is not an address: {e}"))?;
    // 0.0.0.0 means "every interface" to a listener and nothing to a client.
    let target = if addr.ip().is_unspecified() {
        SocketAddr::new(
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            addr.port(),
        )
    } else {
        addr
    };

    if std::env::var("OPENCALC_TLS_CERT").is_ok() {
        return tokio::net::TcpStream::connect(target)
            .await
            .map(|_| ())
            .map_err(|e| format!("nothing is listening on {target}: {e}"));
    }

    let response = reqwest::Client::new()
        .get(format!("http://{target}/healthz"))
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
        .map_err(|e| format!("could not reach {target}: {e}"))?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("{target} answered {}", response.status()))
    }
}

async fn start() -> Result<(), String> {
    let exposure = read_exposure()?;
    for warning in exposure.warnings() {
        // Said once, out loud. None of these will ever *fail*, which is exactly
        // why nothing else would mention them.
        tracing::warn!("{warning}");
    }
    if let Some(node) = &exposure.node {
        let problems = node.problems();
        if !problems.is_empty() {
            // These cannot work, as opposed to probably-should-not, so they
            // stop the process rather than warning it. Every symptom of getting
            // them wrong — peers that never appear, a leader never elected —
            // points somewhere else entirely.
            return Err(format!(
                "this node cannot be found by its peers: {problems:?}"
            ));
        }
    }

    // Built before the listener opens, so a cluster that cannot reach its
    // coordinator refuses to start. The alternative is worse than it sounds: a
    // node that comes up believing it is in a cluster it cannot see will take
    // leadership of every document it is asked about and be wrong about all of
    // them, and the symptom is divergence somewhere else entirely.
    let membership = read_membership(exposure.node.as_ref()).await?;
    tracing::info!(
        coordination = if membership.is_some() {
            "redis"
        } else {
            "standalone"
        },
        "coordination"
    );

    let transport = Arc::new(
        HttpTransport::new(HttpConfig {
            timeout: std::time::Duration::from_millis(env_u64("OPENCALC_HTTP_TIMEOUT_MS", 30_000)),
            max_document_bytes: env_u64("OPENCALC_MAX_DOCUMENT_BYTES", 256 * 1024 * 1024),
        })
        .map_err(|e| format!("could not build the HTTP client: {e}"))?,
    );

    let config = ServiceConfig {
        bind: exposure.public.bind,
        verifier: read_verifier().await?,
        save: SavePolicy::default(),
        snapshots: SnapshotPolicy::default(),
        fetch: Arc::clone(&transport) as Arc<_>,
        deliver: transport as Arc<_>,
        limits: read_limits(),
        membership,
    };

    tracing::info!(
        bind = %config.bind,
        tls = exposure.public.is_tls(),
        node = exposure.node.as_ref().map_or("standalone", |n| n.id.as_str()),
        "starting"
    );
    serve(config).await.map_err(|e| e.to_string())
}

/// Join the cluster this deployment is configured for, if any.
///
/// Standalone is a **first-class mode** (ADR-012), not a degraded one — one
/// process, leader of every document by definition, and a network round trip to
/// agree with itself would be pure cost — so no Redis is entirely normal and
/// says so quietly.
async fn read_membership(node: Option<&NodeIdentity>) -> Result<Option<Membership>, String> {
    let url = std::env::var("OPENCALC_REDIS_URL").ok();
    match (url, node) {
        (Some(url), node) => {
            let namespace = std::env::var("OPENCALC_REDIS_NAMESPACE").unwrap_or_else(|_| {
                casual_calc_collab_server::cluster::redis::DEFAULT_NAMESPACE.to_owned()
            });
            let store = Redis::connect_within(&url, &namespace)
                .await
                .map_err(|e| format!("{e}; set OPENCALC_REDIS_URL to a reachable server"))?;
            Ok(Some(Membership {
                // A node with a Redis but no declared identity is still one node
                // among others. It needs a name to hold a lease under, and one
                // derived from the process is better than refusing to start over
                // a field that otherwise only matters to logs.
                node: node.map_or_else(|| format!("node-{}", std::process::id()), |n| n.id.clone()),
                store: Arc::new(store),
                lease_ms: env_u64("OPENCALC_LEASE_MS", 6_000),
                advertise: node.map_or_else(String::new, |n| n.advertise.to_string()),
            }))
        }
        // A node with an identity and nowhere to announce it is not in a
        // cluster; it is a standalone node that believes otherwise, which is
        // the most expensive way to be wrong here. Its peers never see it, no
        // lease is ever contended, and every node happily leads everything.
        (None, Some(_)) => Err(
            "OPENCALC_NODE_ID is set, so this node expects to be in a cluster, but \
             OPENCALC_REDIS_URL is not: there is nowhere to announce itself or take a lease"
                .to_owned(),
        ),
        (None, None) => Ok(None),
    }
}

fn read_exposure() -> Result<Exposure, String> {
    let public = endpoint("OPENCALC_BIND", "0.0.0.0:8443", "OPENCALC_TLS")?;
    let internal = match std::env::var("OPENCALC_INTERNAL_BIND") {
        Ok(_) => Some(endpoint(
            "OPENCALC_INTERNAL_BIND",
            "0.0.0.0:9443",
            "OPENCALC_INTERNAL_TLS",
        )?),
        Err(_) => None,
    };

    let node = match (
        std::env::var("OPENCALC_NODE_ID"),
        std::env::var("OPENCALC_ADVERTISE"),
    ) {
        (Ok(id), Ok(advertise)) => Some(NodeIdentity {
            id,
            // Not parsed as an IP: a service name is how nodes reach each
            // other on a compose or Kubernetes network. `problems()` checks the
            // shape.
            advertise,
        }),
        // Both or neither: half an identity is a cluster that cannot form, and
        // guessing the other half is how a node advertises an address nobody
        // can reach.
        (Ok(_), Err(_)) | (Err(_), Ok(_)) => {
            return Err(
                "OPENCALC_NODE_ID and OPENCALC_ADVERTISE must be set together, or neither"
                    .to_owned(),
            );
        }
        (Err(_), Err(_)) => None,
    };

    Ok(Exposure {
        public,
        internal,
        proxy: read_proxy_trust()?,
        node,
    })
}

fn endpoint(bind_var: &str, default: &str, tls_prefix: &str) -> Result<Endpoint, String> {
    let bind: SocketAddr = std::env::var(bind_var)
        .unwrap_or_else(|_| default.to_owned())
        .parse()
        .map_err(|e| format!("{bind_var} is not an address: {e}"))?;

    let cert = std::env::var(format!("{tls_prefix}_CERT")).ok();
    let key = std::env::var(format!("{tls_prefix}_KEY")).ok();
    let mut endpoint = match (cert, key) {
        (Some(cert), Some(key)) => Endpoint::secured(bind, cert.into(), key.into()),
        (None, None) => Endpoint::plain(bind),
        // One without the other is a typo, and starting plain because a
        // certificate path was misspelled is the failure nobody notices.
        _ => {
            return Err(format!(
                "{tls_prefix}_CERT and {tls_prefix}_KEY must be set together, or neither"
            ));
        }
    };
    if let Ok(ca) = std::env::var(format!("{tls_prefix}_CLIENT_CA")) {
        endpoint = endpoint.requiring_client_certificate(ca.into());
    }
    Ok(endpoint)
}

fn read_proxy_trust() -> Result<ProxyTrust, String> {
    if env_flag("OPENCALC_TRUST_ANY_PROXY") {
        return Ok(ProxyTrust {
            proxies: Vec::new(),
            trust_any_peer: true,
        });
    }
    let listed = std::env::var("OPENCALC_TRUSTED_PROXIES").unwrap_or_default();
    let mut proxies = Vec::new();
    for entry in listed.split(',').map(str::trim).filter(|e| !e.is_empty()) {
        proxies.push(
            entry
                .parse()
                .map_err(|e| format!("OPENCALC_TRUSTED_PROXIES entry {entry:?}: {e}"))?,
        );
    }
    Ok(ProxyTrust::behind(proxies))
}

async fn read_verifier() -> Result<Verifier, String> {
    let policy = TokenPolicy {
        audience: std::env::var("OPENCALC_AUDIENCE").unwrap_or_default(),
        leeway_secs: env_u64("OPENCALC_TOKEN_LEEWAY_SECS", 60),
        allowed_hosts: std::env::var("OPENCALC_ALLOWED_HOSTS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|h| !h.is_empty())
            .map(str::to_owned)
            .collect(),
        require_https: !env_flag("OPENCALC_ALLOW_PLAIN_CALLBACKS"),
    };

    // Asymmetric first, because it is the one to use: this server can then
    // verify a token and cannot mint one.
    if let Ok(url) = std::env::var("OPENCALC_JWKS_URL") {
        let body = reqwest::get(&url)
            .await
            .map_err(|e| format!("could not fetch {url}: {e}"))?
            .bytes()
            .await
            .map_err(|e| format!("could not read {url}: {e}"))?;
        let keys = KeySet::from_jwks(&body, &[Signing::Rs256, Signing::Es256])
            .map_err(|e| format!("{url}: {e}"))?;
        tracing::info!(keys = keys.len(), %url, "loaded signing keys");
        return Ok(Verifier { policy, keys });
    }

    if let Ok(secret) = std::env::var("OPENCALC_SHARED_SECRET") {
        tracing::warn!(
            "using a shared secret: this process can mint tokens as well as check them, \
             which is what OPENCALC_JWKS_URL avoids"
        );
        return Ok(Verifier {
            policy,
            keys: KeySet::shared_secret(secret.as_bytes()),
        });
    }

    Err(
        "set OPENCALC_JWKS_URL (preferred) or OPENCALC_SHARED_SECRET: without one, \
         no token can be verified and nobody can join"
            .to_owned(),
    )
}

fn read_limits() -> Limits {
    let d = Limits::default();
    Limits {
        max_documents: env_u64("OPENCALC_MAX_DOCUMENTS", d.max_documents as u64) as usize,
        max_participants: env_u64("OPENCALC_MAX_PARTICIPANTS", d.max_participants as u64) as usize,
        max_message_bytes: env_u64("OPENCALC_MAX_MESSAGE_BYTES", d.max_message_bytes as u64)
            as usize,
        idle_eviction_ms: env_u64("OPENCALC_IDLE_EVICTION_MS", d.idle_eviction_ms),
        tick_ms: env_u64("OPENCALC_TICK_MS", d.tick_ms),
        presence_ttl_ms: env_u64("OPENCALC_PRESENCE_TTL_MS", d.presence_ttl_ms),
        client_ping_ms: env_u64("OPENCALC_CLIENT_PING_MS", d.client_ping_ms),
        client_idle_ms: env_u64("OPENCALC_CLIENT_IDLE_MS", d.client_idle_ms),
        drain_timeout_ms: env_u64("OPENCALC_DRAIN_TIMEOUT_MS", d.drain_timeout_ms),
    }
}

/// A number from the environment, or the default.
///
/// An unreadable value takes the default and says so, rather than refusing to
/// start: a typo in a tuning knob should not take a service down, and silence
/// would leave an operator wondering why their setting did nothing.
fn env_u64(name: &str, default: u64) -> u64 {
    match std::env::var(name) {
        Ok(raw) => raw.parse().unwrap_or_else(|_| {
            tracing::warn!(%name, value = %raw, default, "not a number; using the default");
            default
        }),
        Err(_) => default,
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}
