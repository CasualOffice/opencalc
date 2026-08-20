//! Where the coordinator's primary is, asked rather than assumed.
//!
//! [ADR-020](../../../../docs/77-COORDINATOR-AVAILABILITY.md) chose **Sentinel**
//! over Cluster mode, and the reason is that nothing this system asks of Redis
//! needs sharding: the append script names two keys for one document, a lease is
//! one key, and pub/sub is fire-and-forget. What was missing was not capacity —
//! it was something that promotes a replica when the primary goes and tells
//! clients where the primary is now.
//!
//! # The address is a question, not a setting
//!
//! With one Redis the URL *is* the coordinator, and
//! [`ConnectionManager`](redis::aio::ConnectionManager) re-dialling that address
//! is the whole recovery story. With Sentinel the address is an **answer that
//! expires**: after a failover the node at the old address is either gone or —
//! worse, because it looks healthy — demoted to a replica, which answers every
//! write with `READONLY` and never closes the socket. `redis`'s own retry
//! classification calls `READONLY` `NoRetry`, so a `ConnectionManager` pointed at
//! a demoted primary will re-dial nothing, succeed at connecting, and fail every
//! claim and every append for the life of the process.
//!
//! So this module is the *question*: a set of sentinels and a service name,
//! parsed from one URL, re-asked whenever the answer stops working.
//!
//! # What a sentinel URL cannot carry
//!
//! `redis` 0.27 builds the connection to the resolved primary through
//! [`redis::Client::open`], which has no way to accept a private CA or a client
//! certificate — [`redis::Client::build_with_tls`] takes a URL, and the URL here
//! names sentinels rather than the primary. A private CA configured alongside a
//! sentinel URL would therefore be **silently ignored**, and a link that reads as
//! mutually authenticated would be neither. [`super::redis::link_problems`]
//! refuses that combination by name rather than letting it start.

use redis::sentinel::{Sentinel, SentinelNodeConnectionInfo};

/// The URL scheme that means "ask these sentinels", in clear.
pub(crate) const PLAIN_SCHEME: &str = "redis+sentinel://";

/// The same, with TLS to the sentinels and to the primary they name.
pub(crate) const SECURED_SCHEME: &str = "rediss+sentinel://";

/// Whether `url` asks for sentinel resolution at all.
#[must_use]
pub(crate) fn is_sentinel_url(url: &str) -> bool {
    url.starts_with(PLAIN_SCHEME) || url.starts_with(SECURED_SCHEME)
}

/// A parsed `redis+sentinel://` URL: whom to ask, and what to ask about.
///
/// `redis+sentinel://[user[:password]@]host:port[,host:port…]/service[/db]`
///
/// The credentials are applied to **both** the sentinels and the primary they
/// name. Deployments where the two differ are not expressible here, and that is
/// a named limit rather than a silent one — a password that applied to only one
/// of them would produce an authentication failure against whichever half was
/// not configured, at a moment nobody is watching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Every sentinel to try, in the order the URL listed them.
    pub sentinels: Vec<String>,
    /// The `sentinel monitor <name>` this deployment's primary is known by.
    pub service: String,
    /// The database index, if the URL named one.
    pub db: i64,
    /// The username, for a Redis with an ACL rather than a shared password.
    pub username: Option<String>,
    /// The password, for both the sentinels and the primary.
    pub password: Option<String>,
    /// Whether the sentinels and the primary are dialled over TLS.
    pub secured: bool,
}

impl Target {
    /// Parse a sentinel URL, or say what is wrong with it.
    ///
    /// # Errors
    ///
    /// A description in the operator's terms. Every one of these otherwise
    /// surfaces later as a connection failure, and an operator chasing a network
    /// problem will not find a missing service name.
    pub fn parse(url: &str) -> Result<Self, String> {
        let secured = url.starts_with(SECURED_SCHEME);
        let rest = url
            .strip_prefix(if secured {
                SECURED_SCHEME
            } else {
                PLAIN_SCHEME
            })
            .ok_or_else(|| {
                format!("{url:?} is not a sentinel URL: it must begin {PLAIN_SCHEME} or {SECURED_SCHEME}")
            })?;
        // The fragment is dropped the way a browser drops one, and the query is
        // refused rather than ignored: a parameter somebody believed was read is
        // the shape of misconfiguration this whole module exists to make loud.
        let rest = rest.split('#').next().unwrap_or(rest);
        if let Some((_, query)) = rest.split_once('?') {
            return Err(format!(
                "the sentinel URL carries {query:?}, and nothing here reads query parameters; \
                 the form is {PLAIN_SCHEME}host:port,host:port/service"
            ));
        }

        // Split on the **last** `@`, because a password may contain one.
        let (credentials, authority) = match rest.rsplit_once('@') {
            Some((credentials, authority)) => (Some(credentials), authority),
            None => (None, rest),
        };
        let (username, password) = match credentials {
            None => (None, None),
            Some(credentials) => match credentials.split_once(':') {
                Some((user, secret)) => (
                    (!user.is_empty()).then(|| decode(user)),
                    Some(decode(secret)),
                ),
                None => ((!credentials.is_empty()).then(|| decode(credentials)), None),
            },
        };

        let mut path = authority.split('/');
        let hosts = path.next().unwrap_or_default();
        let service = path.next().unwrap_or_default();
        let db = match path.next() {
            None | Some("") => 0,
            Some(db) => db
                .parse::<i64>()
                .map_err(|_| format!("the sentinel URL's database index {db:?} is not a number"))?,
        };
        if path.next().is_some() {
            return Err(format!(
                "the sentinel URL has more path after the database index; the form is \
                 {PLAIN_SCHEME}host:port,host:port/service"
            ));
        }
        if service.is_empty() {
            return Err(format!(
                "the sentinel URL names no service: the primary is found by the name the \
                 sentinels monitor it under, as in {PLAIN_SCHEME}host:26379/mymaster"
            ));
        }

        let mut sentinels = Vec::new();
        for host in hosts.split(',').filter(|h| !h.is_empty()) {
            // A missing port is the one default worth supplying, because 26379
            // is what every sentinel deployment uses and getting it wrong reads
            // as "the sentinels are down".
            let host = if host.rsplit_once(':').is_some_and(|(_, p)| !p.is_empty()) {
                host.to_owned()
            } else {
                format!("{}:26379", host.trim_end_matches(':'))
            };
            sentinels.push(host);
        }
        if sentinels.is_empty() {
            return Err(format!(
                "the sentinel URL names no sentinels; the form is \
                 {PLAIN_SCHEME}host:port,host:port/service"
            ));
        }

        Ok(Self {
            sentinels,
            service: service.to_owned(),
            db,
            username,
            password,
            secured,
        })
    }

    /// The connection URLs for the sentinels themselves.
    ///
    /// A sentinel holds no data, so the database index is deliberately not
    /// carried here: `SELECT` against a sentinel is refused, and sending one
    /// would break resolution for every deployment that uses a database other
    /// than zero.
    fn sentinel_urls(&self) -> Vec<String> {
        let scheme = if self.secured { "rediss" } else { "redis" };
        self.sentinels
            .iter()
            .map(|host| match (&self.username, &self.password) {
                (_, None) => format!("{scheme}://{host}"),
                (None, Some(password)) => format!("{scheme}://:{password}@{host}"),
                (Some(user), Some(password)) => format!("{scheme}://{user}:{password}@{host}"),
            })
            .collect()
    }

    /// How to dial the primary once the sentinels have named it.
    fn node(&self) -> SentinelNodeConnectionInfo {
        SentinelNodeConnectionInfo {
            // `Insecure` is never produced here. It is what `rediss://…/#insecure`
            // asks for on the direct path, and that is refused by name; a
            // sentinel URL must not become the way round it.
            tls_mode: self.secured.then_some(redis::TlsMode::Secure),
            redis_connection_info: Some(redis::RedisConnectionInfo {
                db: self.db,
                username: self.username.clone(),
                password: self.password.clone(),
                ..Default::default()
            }),
        }
    }

    /// Build the resolver this target is asked through.
    ///
    /// # Errors
    ///
    /// [`String`] when a sentinel address is not one `redis` can dial. Checked
    /// here rather than at first use, so a misspelled host stops the process at
    /// startup rather than appearing as a failover that does not happen.
    pub(crate) fn resolver(&self) -> Result<Resolver, String> {
        let sentinel = Sentinel::build(self.sentinel_urls()).map_err(|e| {
            // Never the URLs: they carry the coordinator's password.
            format!("the sentinel addresses in OPENCALC_REDIS_URL cannot be dialled: {e}")
        })?;
        Ok(Resolver {
            sentinel,
            service: self.service.clone(),
            node: self.node(),
        })
    }
}

/// Percent-decoding, for the credential fields only.
///
/// A password is the one part of a URL that routinely contains `@`, `/` and `:`,
/// and an operator who escaped them correctly must not have the escapes handed
/// to Redis verbatim.
fn decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let decoded = (bytes[i] == b'%' && i + 2 < bytes.len())
            .then(|| u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?, 16).ok())
            .flatten();
        match decoded {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The live question: which node is the primary right now.
///
/// Holds its own connections to the sentinels — `redis`'s [`Sentinel`] caches
/// one per sentinel and reuses it — which is why this is behind a mutex rather
/// than cloned: two documents failing over at the same instant should ask once
/// between them, not twice.
pub(crate) struct Resolver {
    sentinel: Sentinel,
    service: String,
    node: SentinelNodeConnectionInfo,
}

impl core::fmt::Debug for Resolver {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // The service name only. Everything else in here is credentials.
        f.debug_struct("Resolver")
            .field("service", &self.service)
            .finish_non_exhaustive()
    }
}

impl Resolver {
    /// Ask the sentinels for a client on whichever node leads now.
    ///
    /// # Errors
    ///
    /// [`super::Unavailable`] when no sentinel answers, or when the ones that do
    /// agree that the service has no reachable primary. Both are "I could not
    /// ask", which is the distinction [`super::Unavailable`] exists to keep:
    /// a node that cannot find the primary does not know whether it leads
    /// anything and must refuse rather than proceed.
    pub(crate) async fn primary(&mut self) -> Result<redis::Client, super::Unavailable> {
        self.sentinel
            .async_master_for(&self.service, Some(&self.node))
            .await
            .map_err(|e| {
                super::Unavailable(format!(
                    "the sentinels were asked which node leads {:?} and could not say: {e}",
                    self.service
                ))
            })
    }

    /// The service name, for a log line that must not print a password.
    pub(crate) fn service(&self) -> &str {
        &self.service
    }
}
