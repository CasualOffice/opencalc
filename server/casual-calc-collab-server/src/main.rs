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
use casual_calc_collab_server::verify::{KeySet, Signing, Trust, Verifier};
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
    // Two questions, not one. `--healthcheck` asks "is this process serving?"
    // and a `no` should restart it; `--readycheck` asks "should this node be
    // given work?" and a `no` should drain it and leave it running. Answering
    // the second with the first is what let a node that had lost the
    // coordinator keep receiving edits it could only refuse (DEP-04).
    let path = if std::env::args().any(|a| a == "--readycheck") {
        Some("/readyz")
    } else if std::env::args().any(|a| a == "--healthcheck") {
        Some("/healthz")
    } else {
        None
    };
    if let Some(path) = path {
        return match probe(path).await {
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
/// Fetches `path` over whichever scheme the listener is configured for, which
/// proves the whole request path works rather than that something is holding
/// the port.
///
/// **The TLS leg does not verify the certificate, and that is deliberate.** A
/// certificate is issued for the hostname an operator's clients use; this
/// connects to loopback inside the container, so verification could only ever
/// fail. What it still establishes is the part that matters here: that a TLS
/// handshake completes and the request path answers. It was previously a bare
/// TCP connect for this case, which established neither — so a listener serving
/// *plaintext* while configured for TLS passed its own health check, and that
/// is exactly the state DEP-01 left every deployment in. A liveness probe is not
/// trying to authenticate the thing it is probing; it is asking whether this
/// process is serving what it claims to serve.
async fn probe(path: &str) -> Result<(), String> {
    let (target, secure) = probe_target()?;
    ask(target, secure, path).await
}

/// Where to probe, and over which scheme, from the same environment the server
/// read — so the check cannot drift from the thing it checks.
fn probe_target() -> Result<(SocketAddr, bool), String> {
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

    Ok((target, std::env::var("OPENCALC_TLS_CERT").is_ok()))
}

/// Fetch one probe path and turn its status into an exit code's worth of answer.
async fn ask(target: SocketAddr, secure: bool, path: &str) -> Result<(), String> {
    let scheme = if secure { "https" } else { "http" };
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(secure)
        .build()
        .map_err(|e| format!("could not build a client: {e}"))?;

    let response = client
        .get(format!("{scheme}://{target}{path}"))
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
        .map_err(|e| format!("could not reach {target} over {scheme}: {e}"))?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("{target}{path} answered {}", response.status()))
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

    // Built before the listener opens, so a certificate that cannot be read or
    // parsed refuses to start the process. The alternative — discovering it on
    // the first connection — is a node that is up, healthy and unusable.
    let tls = match exposure.public.tls.as_ref() {
        None => None,
        Some(_) => Some(std::sync::Arc::new(
            casual_calc_collab_server::net::tls_config(&exposure.public)?,
        )),
    };

    warn_unread_secret_files();

    let config = ServiceConfig {
        bind: exposure.public.bind,
        tls,
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
        // What was **built**, not what was configured. These were the same
        // expression before DEP-01 and did not mean the same thing: the socket
        // was plain while this line said `tls = true`, so the one place an
        // operator looks to check agreed with the mistake.
        tls = config.tls.is_some(),
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
    // A secret, not merely a setting: `redis://user:pass@host` puts the
    // password in the URL, so this needs the same `_FILE` form as the signing
    // key (`DEP-11`).
    let url = casual_calc_secrets::env_secret("OPENCALC_REDIS_URL").map_err(|w| w.to_string())?;
    match (url, node) {
        (Some(url), node) => {
            let namespace = std::env::var("OPENCALC_REDIS_NAMESPACE").unwrap_or_else(|_| {
                casual_calc_collab_server::cluster::redis::DEFAULT_NAMESPACE.to_owned()
            });
            let tls = read_link_tls()?;
            for warning in casual_calc_collab_server::cluster::redis::link_warnings(&url) {
                // The same treatment `Exposure::warnings` gets, for the one
                // connection that is not a listener. Nothing here will ever
                // fail, which is why nothing else would mention it.
                tracing::warn!("{warning}");
            }
            let policy = casual_calc_collab_server::cluster::redis::LinkPolicy {
                tls,
                min_replicas: read_min_replicas()?,
            };
            let store = Redis::connect_under(&url, &namespace, &policy)
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
                // Optional, and doing nothing without it is the honest
                // outcome: a deployment behind one load balancer has no
                // per-node public address, and a redirect would come back
                // through the balancer to an arbitrary node (`DEP-09`).
                public_url: std::env::var("OPENCALC_PUBLIC_URL")
                    .ok()
                    .map(|u| u.trim().to_owned())
                    .filter(|u| !u.is_empty()),
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

/// How much redundancy this node insists its coordinator is configured for.
///
/// **Unset means unchecked**, which is the pre-ADR-020 behaviour and stays the
/// default: a single-node coordinator has no replicas to be in sync with, and
/// refusing to start against one would replace a named risk with a new outage.
///
/// Set to `1` on a replicated coordinator and the node refuses to use a primary
/// that is not itself configured with `min-replicas-to-write` at least that
/// high — at startup **and after every failover**, because the setting is per
/// server and the mistake it catches is the one where only the original primary
/// was configured. Without it, Redis's asynchronous replication means a
/// promotion can silently drop an append this node already told a client was
/// saved, which is the one thing ADR-014 §4 promises will not happen.
fn read_min_replicas() -> Result<u32, String> {
    match std::env::var("OPENCALC_REDIS_MIN_REPLICAS") {
        Err(_) => Ok(0),
        // Refused rather than defaulted. Every other numeric setting here warns
        // and carries on, and that is right for a budget; this one decides
        // whether a durability floor is enforced at all, and "0 because it was
        // misspelt" is the failure it exists to prevent.
        Ok(raw) => raw.trim().parse::<u32>().map_err(|_| {
            format!(
                "OPENCALC_REDIS_MIN_REPLICAS is {raw:?}, which is not a number of replicas;                  unset it to leave the coordinator's durability unchecked"
            )
        }),
    }
}

/// What this node presents to, and accepts from, the coordinator.
///
/// Three variables and one rule between two of them, which is the same rule
/// [`endpoint`] enforces for a listener's certificate: a keypair is both files
/// or neither, because starting without a client certificate because its path
/// was misspelled is the failure nobody notices until the coordinator starts
/// refusing this node.
fn read_link_tls() -> Result<casual_calc_collab_server::cluster::redis::LinkTls, String> {
    let cert = std::env::var("OPENCALC_REDIS_CLIENT_CERT").ok();
    let key = std::env::var("OPENCALC_REDIS_CLIENT_KEY").ok();
    let client = match (cert, key) {
        (Some(cert), Some(key)) => Some(casual_calc_collab_server::config::TlsFiles {
            certificate: cert.into(),
            key: key.into(),
        }),
        (None, None) => None,
        _ => {
            return Err(
                "OPENCALC_REDIS_CLIENT_CERT and OPENCALC_REDIS_CLIENT_KEY must be set \
                 together, or neither"
                    .to_owned(),
            );
        }
    };
    Ok(casual_calc_collab_server::cluster::redis::LinkTls {
        root_ca: std::env::var("OPENCALC_REDIS_CA").ok().map(Into::into),
        client,
    })
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

/// Secrets that appear in this repository, and so are not secrets.
///
/// A list rather than one string: the value somebody copies is whichever
/// placeholder they happened to read, so every one that has ever shipped in an
/// example belongs here.
const PLACEHOLDER_SECRETS: &[&str] = &[
    "dev-secret-change-me",
    "change-me",
    "changeme",
    // What `.env.example` ships, so `cp .env.example .env` leaves a key that
    // does not work rather than one that works and is public.
    "change-me-before-anyone-else-can-reach-this",
    // No longer used by the suite, and kept because it is published: somebody
    // who copied it once would otherwise keep a working weak key.
    "browser-tests-shared-secret",
];

/// The secrets this server reads, in their file form.
///
/// Literal rather than built from the base names, for two reasons: the
/// deployment page is checked against the strings that appear in server
/// sources, and a `_FILE` variable nobody names is one an operator can be told
/// about only by accident.
const SECRET_FILES: &[&str] = &["OPENCALC_SHARED_SECRET_FILE", "OPENCALC_REDIS_URL_FILE"];

/// Say so when the environment holds a `_FILE` variable nothing here reads.
///
/// A mount that is present, correct and ignored otherwise looks exactly like a
/// server that was never given a secret.
fn warn_unread_secret_files() {
    for name in casual_calc_secrets::unknown_secret_files(
        std::env::vars().map(|(name, _)| name),
        SECRET_FILES,
    ) {
        tracing::warn!(
            %name,
            reads = ?SECRET_FILES,
            "a *_FILE variable is set that this server does not read; check the spelling"
        );
    }
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

    // Multi-tenant first, because it is the only configuration in which the
    // issuer is a *boundary*: one key set per issuer, and a token is checked
    // against the keys of the issuer it names. With a single shared key set,
    // `iss` is a label — any tenant holding a key in that set can sign a token
    // naming any other tenant, which is what `DEP-10` records.
    if let Ok(spec) = std::env::var("OPENCALC_ISSUERS") {
        let accepted = vec![Signing::Rs256, Signing::Es256];
        let mut trusts = Vec::new();
        for entry in spec.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let Some((issuer, url)) = entry.split_once('=') else {
                return Err(format!(
                    "OPENCALC_ISSUERS entry {entry:?} is not `issuer=https://…/jwks.json`"
                ));
            };
            let (issuer, url) = (issuer.trim(), url.trim());
            if issuer.is_empty() {
                return Err(format!("OPENCALC_ISSUERS entry {entry:?} names no issuer"));
            }
            let keys = casual_calc_collab_server::verify::fetch_keys(url, &accepted).await?;
            tracing::info!(keys = keys.len(), %url, %issuer, "loaded signing keys");
            trusts.push(Trust::refreshing(
                issuer,
                keys,
                url.to_owned(),
                accepted.clone(),
                env_u64("OPENCALC_JWKS_MIN_REFRESH_MS", 10_000),
            ));
        }
        if trusts.is_empty() {
            return Err("OPENCALC_ISSUERS is set but names no issuer".to_owned());
        }
        return Ok(Verifier::tenanted(policy, trusts));
    }

    // Asymmetric first, because it is the one to use: this server can then
    // verify a token and cannot mint one.
    if let Ok(url) = std::env::var("OPENCALC_JWKS_URL") {
        let accepted = vec![Signing::Rs256, Signing::Es256];
        let keys = casual_calc_collab_server::verify::fetch_keys(&url, &accepted).await?;
        tracing::info!(keys = keys.len(), %url, "loaded signing keys");
        // Refreshing, not fixed. The first fetch is a starting point, not the
        // answer for the life of the process — see `JwksSource`.
        let min_refresh = env_u64("OPENCALC_JWKS_MIN_REFRESH_MS", 10_000);
        // One key set, but the issuer can still be pinned. That does not make
        // the deployment multi-tenant — it cannot, with one key set — it only
        // refuses a token minted for somebody else by the same signer.
        return Ok(match std::env::var("OPENCALC_ISSUER") {
            Ok(issuer) if !issuer.trim().is_empty() => Verifier::tenanted(
                policy,
                vec![Trust::refreshing(
                    issuer.trim(),
                    keys,
                    url,
                    accepted,
                    min_refresh,
                )],
            ),
            _ => Verifier::refreshing(policy, keys, url, accepted, min_refresh),
        });
    }

    if let Some(secret) =
        casual_calc_secrets::env_secret("OPENCALC_SHARED_SECRET").map_err(|why| why.to_string())?
    {
        // Refused rather than warned about. With a shared secret the holder can
        // *mint* tokens, not merely check them — so a placeholder that appears
        // in a public compose file is a key to every document of every
        // deployment that forgot to change it. A default that works is a
        // default that ships, and the failure of shipping it is silent
        // (SEC-003).
        if PLACEHOLDER_SECRETS.contains(&secret.as_str()) {
            return Err(format!(
                "OPENCALC_SHARED_SECRET is still {secret:?}, which is published in this \
                 repository. Anybody holding it can mint a token for any document. \
                 Set a real one, or use OPENCALC_JWKS_URL."
            ));
        }
        // Length is not strength, but a two-character secret is not a mistake
        // anybody makes deliberately.
        if secret.len() < 16 {
            return Err(format!(
                "OPENCALC_SHARED_SECRET is {} bytes; at least 16 are needed for a signing key",
                secret.len()
            ));
        }
        tracing::warn!(
            "using a shared secret: this process can mint tokens as well as check them, \
             which is what OPENCALC_JWKS_URL avoids"
        );
        let keys = KeySet::shared_secret(secret.as_bytes());
        return Ok(match std::env::var("OPENCALC_ISSUER") {
            Ok(issuer) if !issuer.trim().is_empty() => {
                Verifier::tenanted(policy, vec![Trust::fixed(issuer.trim(), keys)])
            }
            _ => Verifier::fixed(policy, keys),
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
        jwks_refresh_ms: env_u64("OPENCALC_JWKS_REFRESH_MS", d.jwks_refresh_ms),
        max_pending_connections: env_u64(
            "OPENCALC_MAX_PENDING_CONNECTIONS",
            d.max_pending_connections as u64,
        ) as usize,
        // The ceiling is the container's, discovered rather than configured —
        // a node with a 2 GB cgroup limit on a 64 GB host is a 2 GB node, and
        // sizing from the host is how a container is killed while `free` looks
        // healthy. `OPENCALC_MEMORY_LIMIT_BYTES` overrides it for a deployment
        // that knows better than its cgroup.
        memory_budget: env_u64("OPENCALC_MEMORY_LIMIT_BYTES", 0)
            .checked_sub(0)
            .filter(|v| *v > 0)
            .or_else(casual_calc_collab_server::memory::container_limit_bytes)
            .map(
                |limit_bytes| casual_calc_collab_server::memory::MemoryBudget {
                    limit_bytes,
                    high_water_percent: env_u64("OPENCALC_MEMORY_HIGH_WATER_PERCENT", 85)
                        .clamp(50, 95),
                },
            ),
        join_timeout_ms: env_u64("OPENCALC_JOIN_TIMEOUT_MS", d.join_timeout_ms),
        drain_timeout_ms: env_u64("OPENCALC_DRAIN_TIMEOUT_MS", d.drain_timeout_ms),
        // Must fit the orchestrator's stop grace. Named so an operator who
        // raises `stop_grace_period` can raise this to match, and so one who
        // lowers it finds the knob rather than a hardcoded number.
        drain_deadline_ms: env_u64("OPENCALC_DRAIN_DEADLINE_MS", d.drain_deadline_ms),
        drain_concurrency: env_u64("OPENCALC_DRAIN_CONCURRENCY", d.drain_concurrency as u64)
            as usize,
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

#[cfg(test)]
mod secret_tests {
    use super::PLACEHOLDER_SECRETS;

    /// Every placeholder this repository has ever shipped must be refused.
    ///
    /// A shared secret lets its holder **mint** tokens, not merely check them,
    /// so a value printed in a public compose file is a key to every document
    /// of every deployment that did not change it. It used to be the compose
    /// default, which meant `docker compose up` produced a working deployment
    /// secured by a string in this repository — and nothing said so, because a
    /// default that works is a default nobody revisits (SEC-003).
    #[test]
    fn the_values_this_repository_publishes_are_all_listed() {
        // Read out of the files rather than typed here: a new placeholder added
        // to an example is exactly the case this must not miss.
        let sources = [
            include_str!("../../../.env.example"),
            include_str!("../../../docker-compose.yml"),
            include_str!("../../../docker-compose.cluster.yml"),
        ];
        for text in sources {
            for line in text.lines() {
                let Some((_, rest)) = line.split_once("OPENCALC_SHARED_SECRET") else {
                    continue;
                };
                // The *variable*, not anything merely prefixed by it.
                // `OPENCALC_SHARED_SECRET_FILE` names a path to read the secret
                // from, and a path is not a secret — matching on the prefix
                // read `/run/secrets/…` as a published signing key. Narrowing
                // this does not narrow what it catches: an assignment still has
                // a separator straight after the name.
                if !rest.starts_with(['=', ':', ' ']) {
                    continue;
                }
                let value = rest
                    .trim_start_matches(['=', ':', ' '])
                    .split(['#', '}'])
                    .next()
                    .unwrap_or("")
                    .trim();
                // Compose reads it from the environment; only a literal is a
                // published value.
                if value.is_empty() || value.starts_with('$') {
                    continue;
                }
                assert!(
                    PLACEHOLDER_SECRETS.contains(&value),
                    "{value:?} is published here and would be accepted as a signing key"
                );
            }
        }
    }
}

#[cfg(test)]
mod probe_tests {
    use super::ask;
    use std::net::SocketAddr;

    /// A server that answers the two probe paths differently — which is the
    /// whole point of there being two.
    async fn two_probes() -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new()
            .route("/healthz", axum::routing::get(|| async { "ok\n" }))
            .route(
                "/readyz",
                axum::routing::get(|| async {
                    (axum::http::StatusCode::SERVICE_UNAVAILABLE, "not ready\n")
                }),
            );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        addr
    }

    /// **The two probes are asked separately and can disagree.**
    ///
    /// The container check used to fetch `/healthz` whatever it was asked, so
    /// `--readycheck` would have reported a drained node as fit for traffic —
    /// the same DEP-04 blindness one layer up, and invisible in any deployment
    /// where the two answers happen to agree, which is all of them until Redis
    /// goes away.
    #[tokio::test]
    async fn readiness_and_liveness_are_asked_separately() {
        let addr = two_probes().await;

        assert!(
            ask(addr, false, "/healthz").await.is_ok(),
            "the process is serving"
        );

        let ready = ask(addr, false, "/readyz").await;
        let why = ready.expect_err("a draining node is not ready");
        assert!(
            why.contains("/readyz") && why.contains("503"),
            "the failure says which probe and what it answered: {why}"
        );
    }
}
