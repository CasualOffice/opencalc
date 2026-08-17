//! Minting the collaboration session token.
//!
//! The adapter is an ordinary OpenCalc integrator: it holds the signing key and
//! tells the collaboration server, per join, where the file is, where the bytes
//! go, who is joining and what they may do. Nothing here is WOPI-specific by
//! the time it reaches the server — which is the point of the design, and why
//! the server needed no changes to gain a WOPI integration.
//!
//! # Both URLs point back at this service
//!
//! Not at the WOPI host. The server fetches from us and saves to us, so the
//! host's access token stays in this process: out of the server's configuration,
//! its logs, and — in a cluster — its shared log. The extra hop is the price of
//! a credential that only one service ever sees.

use crate::config::Config;
use crate::sessions::Session;

/// Who is joining.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct User {
    pub id: String,
    pub name: String,
}

/// What they are joining.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Document {
    pub key: String,
    pub id: String,
    pub title: String,
    pub url: String,
}

/// What they may do once there.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Permissions {
    pub access: String,
}

/// Where the finished bytes go.
///
/// Tagged, because a URL alone does not say whether the server should make an
/// OnlyOffice-style POST or a WOPI `PutFile`. This is deliberately the `url`
/// shape even though the far end is a WOPI host: the WOPI request is made by
/// *this* service, one hop later.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CallbackRef {
    pub kind: String,
    pub url: String,
}

/// Everything signed into one token.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Claims {
    pub iss: String,
    pub aud: String,
    pub exp: u64,
    pub iat: u64,
    pub user: User,
    pub document: Document,
    pub permissions: Permissions,
    pub callback: CallbackRef,
}

/// The claims for one session.
#[must_use]
pub fn claims_for(config: &Config, session: &Session, id: &str, now_secs: u64) -> Claims {
    Claims {
        iss: "opencalc-wopi".to_owned(),
        aud: config.audience.clone(),
        // Bounded by the session, not by a fixed window: the session is what
        // holds the host's access token, and a collaboration token that
        // outlives it names a fetch URL that answers 404.
        exp: now_secs + config.session_ttl_ms / 1000,
        iat: now_secs,
        user: User {
            id: session.user_id.clone(),
            name: session.user_name.clone(),
        },
        document: Document {
            // **The session key is the file, not the session id.** Two people
            // opening the same file arrive from the host with two different
            // access tokens and two adapter sessions; keying on the session id
            // would put them in two documents that both save over each other,
            // which is the exact failure co-editing exists to prevent.
            key: file_key(&session.src),
            id: file_key(&session.src),
            title: session.title.clone(),
            url: format!("{}/wopi/content/{id}", config.internal_url),
        },
        permissions: Permissions {
            access: if session.editable { "edit" } else { "view" }.to_owned(),
        },
        callback: CallbackRef {
            kind: "url".to_owned(),
            url: format!("{}/wopi/callback/{id}", config.internal_url),
        },
    }
}

/// Sign the claims for one session.
#[must_use]
pub fn mint(config: &Config, session: &Session, id: &str, now_secs: u64) -> String {
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &claims_for(config, session, id, now_secs),
        &jsonwebtoken::EncodingKey::from_secret(config.secret.as_bytes()),
    )
    .unwrap_or_default()
}

/// A stable session key for a file, derived from its `WOPISrc`.
///
/// The access token is stripped: it differs per user and per open, and a key
/// that included it would put every participant in a session of their own.
fn file_key(src: &str) -> String {
    let without_token = src.split('?').next().unwrap_or(src);
    // A key travels in a token and appears in logs; a URL with slashes and
    // colons in it is awkward in both. Hex of the bytes is not reversible-proof
    // and is not meant to be — it is a canonical, log-safe spelling.
    let mut key = String::with_capacity(without_token.len());
    for c in without_token.chars() {
        if c.is_ascii_alphanumeric() {
            key.push(c);
        } else {
            key.push('-');
        }
    }
    key
}

#[cfg(test)]
mod tests;
