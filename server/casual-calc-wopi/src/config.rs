//! What this service is told, and what it refuses to start without.
//!
//! # `WOPISrc` is attacker input
//!
//! The action URL is reached by sending a *browser* to it, so anybody who can
//! get a user to click a link chooses the `WOPISrc` this process then fetches.
//! Unrestricted, that is a server-side request forgery with a friendly UI: the
//! link `…/wopi/edit?WOPISrc=http://169.254.169.254/latest/meta-data/` makes
//! this service fetch a cloud metadata endpoint from inside the perimeter.
//!
//! So the hosts we will talk to are an **allow-list, required at startup**. The
//! collaboration server beside this made the same decision for the same reason;
//! the difference is that its URLs arrive in a signed token and these arrive in
//! a query string, which makes the list more important here, not less.

use std::collections::BTreeSet;

/// Everything this service needs to run.
#[derive(Debug, Clone)]
pub struct Config {
    /// Where to listen.
    pub bind: String,
    /// The base URL a *browser* reaches this service on. Goes into the
    /// discovery document, so it is the address the WOPI host will send people
    /// to — behind a proxy it is never the bind address.
    pub public_url: String,
    /// The base URL the *collaboration server* reaches this service on, for the
    /// content fetch and the save callback. Usually a service name on an
    /// internal network.
    pub internal_url: String,
    /// The WebSocket endpoint the browser dials.
    pub collab_url: String,
    /// Where the editor bundle is served from.
    pub editor_url: String,
    /// The HS256 secret shared with the collaboration server, used to mint
    /// session tokens.
    pub secret: String,
    /// The audience those tokens carry.
    pub audience: String,
    /// WOPI hosts this service will talk to. Empty is not allowed.
    pub allowed_hosts: BTreeSet<String>,
    /// Whether a `WOPISrc` may be plain `http`. Local development only.
    pub allow_plain: bool,
    /// How many sessions this node will hold.
    pub max_sessions: usize,
    /// How long a session lives before its lock is released and it is dropped.
    pub session_ttl_ms: u64,
    /// The largest file this service will fetch or hold.
    pub max_document_bytes: u64,
    /// What this deployment calls itself.
    pub brand: crate::discovery::Brand,
}

impl Config {
    /// Read the configuration from the environment.
    ///
    /// # Errors
    ///
    /// A description of what is missing or unusable. Returned rather than
    /// panicked so the process can print one line an operator can act on.
    pub fn from_env() -> Result<Self, String> {
        let public_url = required("OPENCALC_WOPI_PUBLIC_URL")?;
        let secret = required("OPENCALC_SHARED_SECRET")?;
        if secret.len() < 16 {
            return Err(
                "OPENCALC_SHARED_SECRET is shorter than 16 bytes, which is not a secret".to_owned(),
            );
        }

        let allowed_hosts: BTreeSet<String> = std::env::var("OPENCALC_WOPI_ALLOWED_HOSTS")
            .unwrap_or_default()
            .split(',')
            .map(|h| h.trim().to_ascii_lowercase())
            .filter(|h| !h.is_empty())
            .collect();
        if allowed_hosts.is_empty() {
            return Err(concat!(
                "OPENCALC_WOPI_ALLOWED_HOSTS is empty. A WOPISrc arrives in a query string, ",
                "so without a list this service will fetch whatever URL a link puts in front ",
                "of a user. Set it to the WOPI hosts you integrate with."
            )
            .to_owned());
        }

        let internal_url =
            std::env::var("OPENCALC_WOPI_INTERNAL_URL").unwrap_or_else(|_| public_url.clone());

        Ok(Self {
            bind: std::env::var("OPENCALC_WOPI_BIND").unwrap_or_else(|_| "0.0.0.0:8090".to_owned()),
            public_url: public_url.trim_end_matches('/').to_owned(),
            internal_url: internal_url.trim_end_matches('/').to_owned(),
            collab_url: std::env::var("OPENCALC_COLLAB_URL")
                .unwrap_or_else(|_| "/collab".to_owned()),
            editor_url: std::env::var("OPENCALC_EDITOR_URL")
                .unwrap_or_else(|_| "/editor/editor.html".to_owned()),
            secret,
            audience: std::env::var("OPENCALC_AUDIENCE")
                .unwrap_or_else(|_| "opencalc-collab".to_owned()),
            allowed_hosts,
            allow_plain: std::env::var("OPENCALC_WOPI_ALLOW_PLAIN").is_ok(),
            max_sessions: number("OPENCALC_WOPI_MAX_SESSIONS", 500)?,
            session_ttl_ms: number("OPENCALC_WOPI_SESSION_TTL_MS", 8 * 3600 * 1000)?,
            max_document_bytes: number("OPENCALC_WOPI_MAX_DOCUMENT_BYTES", 64 << 20)?,
            brand: crate::discovery::Brand::from_env(),
        })
    }

    /// Whether this service will fetch `src`.
    ///
    /// # Errors
    ///
    /// Why not, in terms an operator can act on — a rejected host names the
    /// host, because the alternative is an administrator who has configured
    /// everything correctly except one hostname and has nothing to go on.
    pub fn permits(&self, src: &str) -> Result<(), String> {
        let rest = if let Some(rest) = src.strip_prefix("https://") {
            rest
        } else if let Some(rest) = src.strip_prefix("http://") {
            if !self.allow_plain {
                return Err("a WOPISrc must be https".to_owned());
            }
            rest
        } else {
            return Err("a WOPISrc must be an absolute http or https URL".to_owned());
        };

        // Authority is everything before the first `/`, `?` or `#`. Taking it
        // by splitting on `/` alone lets `https://evil.example?x=/allowed.host`
        // read as the allowed host.
        let authority = rest
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default()
            // Credentials in a URL are a way to write `user@allowed.host` and
            // have the real authority be somewhere else entirely.
            .rsplit('@')
            .next()
            .unwrap_or_default();
        let host = match authority.rsplit_once(':') {
            // An IPv6 literal keeps its brackets and has colons of its own.
            Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => h,
            _ => authority,
        }
        .to_ascii_lowercase();

        if host.is_empty() {
            return Err("a WOPISrc must name a host".to_owned());
        }
        if self.allowed_hosts.contains(&host) {
            Ok(())
        } else {
            Err(format!("{host} is not in OPENCALC_WOPI_ALLOWED_HOSTS"))
        }
    }
}

fn required(key: &str) -> Result<String, String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{key} is not set"))
}

fn number<T: std::str::FromStr>(key: &str, default: T) -> Result<T, String> {
    match std::env::var(key) {
        Ok(raw) => raw
            .trim()
            .parse()
            .map_err(|_| format!("{key} is not a number: {raw}")),
        Err(_) => Ok(default),
    }
}

#[cfg(test)]
mod tests;
