//! The token: who you are, which document, what you may do, and where the
//! finished bytes go.
//!
//! [ADR-012](../../../docs/57-COLLABORATION-SERVER-BOUNDARY.md) settled that
//! **the token is the whole integration contract**, and
//! [ADR-014](../../../docs/59-COLLABORATION-SERVICE-STACK.md) settled that the
//! integrator signs it and this server only verifies. This module is what the
//! two of them describe, as a type.
//!
//! The consequence worth stating plainly: **the server holds no per-document
//! state**. It does not have a database of who may edit what, or where each
//! file came from, or where it should be sent back to. It is told, per join, by
//! a party that already knows — and cannot be persuaded otherwise, because the
//! claims are signed.
//!
//! # Everything here is checked, not merely carried
//!
//! A permission that is transported and then ignored is worse than none: it
//! reads like a guarantee in the integrator's code and is a suggestion in ours.
//! So [`Access::Comment`] refuses a cell edit at the operation level, not by
//! hiding a toolbar; [`Permissions::download`] is consulted where a file would
//! be produced; and the URLs are checked against a policy before the server
//! will fetch from or post to them.
//!
//! # No I/O
//!
//! Verification is a function of the claims, the configuration and a supplied
//! clock. The signature check needs a key and the key may need fetching, but
//! that belongs to the caller: expiry, audience, document match and URL policy
//! are all decidable without a network, and are the parts whose bugs live in
//! rare cases.

use std::collections::BTreeSet;

use casual_calc_transaction::Operation;
use casual_calc_transaction::protocol::Refusal;

/// What a participant may do with the document.
///
/// Ordered by strength, so `>=` means "at least this much".
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    /// Read it. Selecting, scrolling and — subject to
    /// [`Permissions::copy`] — copying all still work: a viewer you cannot
    /// read out of is hostile, and reading changes nothing.
    View,
    /// Read it and attach comments, but not change a value.
    ///
    /// A real mode rather than a label: see [`Access::permits`], which refuses
    /// a cell edit from a commenter at the operation level.
    Comment,
    /// Change it.
    Edit,
}

impl Access {
    /// Whether an operation is one this access level allows.
    ///
    /// The enforcement point. A commenter may attach, edit and remove comments
    /// and may not touch a value, a style or the shape of the sheet; a viewer
    /// may do none of it. Deliberately a **deny-by-default** match: an
    /// operation added later is refused for anyone below [`Access::Edit`] until
    /// somebody decides which side of the line it belongs on, because the
    /// failure of forgetting is silent and the failure of refusing is a bug
    /// report.
    #[must_use]
    /// The wire spelling of this access.
    ///
    /// An exhaustive match on purpose: adding a variant here without deciding
    /// how it travels will not compile, which is the only thing keeping the two
    /// enums from drifting apart.
    pub fn to_wire(self) -> casual_calc_transaction::protocol::WireAccess {
        use casual_calc_transaction::protocol::WireAccess as W;
        match self {
            Access::View => W::View,
            Access::Comment => W::Comment,
            Access::Edit => W::Edit,
        }
    }

    /// And back.
    pub fn from_wire(wire: casual_calc_transaction::protocol::WireAccess) -> Self {
        use casual_calc_transaction::protocol::WireAccess as W;
        match wire {
            W::View => Access::View,
            W::Comment => Access::Comment,
            W::Edit => Access::Edit,
        }
    }

    /// The more restrictive of two.
    ///
    /// The whole safety property of a session override lives here: the server
    /// takes the minimum of the token and the override, so an override can only
    /// ever *reduce* what a token granted. A client that asks for more gets the
    /// token's answer, which is why a compromised one cannot promote itself.
    pub fn most_restrictive(self, other: Self) -> Self {
        // `View < Comment < Edit`, declared in that order, so `min` is the
        // rule rather than a table somebody has to keep right.
        self.min(other)
    }

    pub fn permits(self, op: &Operation) -> bool {
        match self {
            Access::Edit => true,
            Access::View => false,
            Access::Comment => Self::is_comment_only(op),
        }
    }

    fn is_comment_only(op: &Operation) -> bool {
        match op {
            Operation::SetSheetMetadata { changed, .. } => {
                changed.contains(casual_calc_transaction::SheetFields::COMMENTS)
                    && *changed == casual_calc_transaction::SheetFields::COMMENTS
            }
            // A batch is exactly its members: a commenter may send several
            // comment operations at once and nothing else among them.
            Operation::Batch(ops) => !ops.is_empty() && ops.iter().all(Self::is_comment_only),
            _ => false,
        }
    }
}

/// The capabilities that are orthogonal to [`Access`].
///
/// Separate because they do not line up: a viewer may legitimately be allowed
/// to download, and an editor may legitimately not be — a document open for
/// editing inside a system that keeps it there is a normal arrangement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Permissions {
    /// What the participant may do to the document.
    pub access: Access,
    /// Whether a copy may leave — save-as, export to `.xlsx`/CSV, print to file.
    #[serde(default = "yes")]
    pub download: bool,
    /// Whether printing is offered.
    #[serde(default = "yes")]
    pub print: bool,
    /// Whether the selection may be copied to the clipboard.
    ///
    /// Worth an honest note: this is a **client-side** restriction. The bytes
    /// are on the participant's machine by the time they can see them, and any
    /// system that claims otherwise is describing a screenshot they cannot
    /// prevent. It is here because integrators' policies ask for it and because
    /// honouring it is the difference between a deterrent and nothing.
    #[serde(default = "yes")]
    pub copy: bool,
}

const fn yes() -> bool {
    true
}

impl Default for Permissions {
    fn default() -> Self {
        // The safe default is the least: a token that forgets to say should not
        // thereby grant editing.
        Self {
            access: Access::View,
            download: true,
            print: true,
            copy: true,
        }
    }
}

/// Who is editing.
///
/// Presence reads its display fields from here and from nowhere else — a
/// participant never states its own name (see [`crate::presence`]), because
/// presence is the one surface where a claimed identity would be believed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct User {
    /// Stable identifier in the host's system. Two connections with the same id
    /// are the same person on two devices.
    pub id: String,
    /// Display name, shown on the cursor and in the participant list.
    pub name: String,
    /// Optional, and never required: an integrator that does not want to hand
    /// personal data to an editing service should not have to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// An avatar to show beside the cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    /// A team or department, for grouping a long participant list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// A stable colour for this person's cursor. Absent, one is derived from
    /// the id, which is what keeps somebody the same colour between sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// Which document, and where its bytes come from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Document {
    /// The **editing session** key. Everyone who joins with the same key joins
    /// the same session and sees the same revisions.
    ///
    /// Not the same thing as [`id`](Self::id), and the difference has teeth: to
    /// start a *fresh* session over the same file — after restoring an old
    /// version, or after a save the host wants to be the new baseline — the
    /// host issues a **new key**. Reusing the old one joins the session that is
    /// still running, which is still holding the content the host just
    /// replaced.
    pub key: String,
    /// The file's stable identifier in the host's store. Carried for logging,
    /// telemetry and the callback; never used to decide who may join.
    pub id: String,
    /// The file name, for the window title and for the callback.
    pub title: String,
    /// An opaque version marker from the host, echoed back in the callback so
    /// it can tell which version it is being handed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The id of whoever owns it, if the host tracks that. Presence can mark
    /// them; nothing is decided by it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
    /// Where the server fetches the original package.
    ///
    /// A session starts from the **original file** and never from a model
    /// snapshot (ADR-012), or the retained parts of ADR-007 are silently
    /// dropped the first time anyone opens a document.
    pub url: String,
}

/// Where the finished bytes go when editing stops.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Callback {
    /// Post the package, and the status changes, to one URL. OnlyOffice's
    /// shape, and the one an integrator without WOPI reaches for.
    Url {
        /// Where to POST.
        url: String,
    },
    /// A WOPI host: `PutFile` against `src`, bearing `token`.
    Wopi {
        /// The `WOPISrc` for the file.
        src: String,
        /// The WOPI access token to present.
        token: String,
        /// When that token stops working, as a Unix millisecond timestamp.
        /// Editing past it will fail to save, so the server warns before it
        /// arrives rather than after.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token_expiry_ms: Option<u64>,
    },
}

impl Callback {
    /// The URL the server would contact, whichever shape this is.
    #[must_use]
    pub fn endpoint(&self) -> &str {
        match self {
            Callback::Url { url } => url,
            Callback::Wopi { src, .. } => src,
        }
    }
}

/// Everything a host signs into one token.
///
/// The registered claims are spelled the way JWT spells them, because they are
/// JWT's and renaming them would mean every integrator's signing code needs a
/// translation table.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Claims {
    /// Who signed it. Selects the key set when a server serves several tenants.
    pub iss: String,
    /// Who it was minted for. Checked against this server's configured
    /// audience, so a token issued for another service cannot be replayed here.
    pub aud: String,
    /// When it stops being valid, in Unix **seconds** as JWT specifies.
    ///
    /// Required, with no default. A token without an expiry is a permanent key
    /// to a document, and the one thing an integrator is most likely to leave
    /// out is the one thing that limits the damage when a token leaks.
    pub exp: u64,
    /// When it was issued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iat: Option<u64>,
    /// Not valid before this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nbf: Option<u64>,
    /// A unique id for this token, if the host wants replay tracking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,

    /// Who is joining.
    pub user: User,
    /// What they are joining.
    pub document: Document,
    /// What they may do once there.
    #[serde(default)]
    pub permissions: Permissions,
    /// Whether this participant may reduce other people's access for the life
    /// of the session (`COL-40`).
    ///
    /// **Not inferred from `Access::Edit`.** Every editor being able to lock
    /// every other editor out is a different feature and a worse one, so this
    /// is a claim the host makes deliberately. Absent means false, which is the
    /// safe default and what every existing token says.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub owner: bool,
    /// Where the result goes. Absent means **the server will not save**: a
    /// preview, or a session the host intends to collect by other means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback: Option<Callback>,
}

/// What this server will accept.
#[derive(Debug, Clone, Default)]
pub struct TokenPolicy {
    /// The audience a token must name. Empty accepts any, which is only
    /// sensible in a single-tenant deployment.
    pub audience: String,
    /// Seconds of clock skew tolerated on `exp`/`nbf`.
    ///
    /// Some slack is not laxity: the host and this server keep different
    /// clocks, and rejecting a token that is thirty seconds from valid produces
    /// a login loop nobody can diagnose.
    pub leeway_secs: u64,
    /// Hosts the server may fetch from and post to. Empty allows any.
    ///
    /// Defence in depth, and worth having even though the host signed the URL:
    /// a token names an address this server will connect to, which makes a
    /// leaked or mis-issued token a request-forgery primitive pointed at
    /// whatever the server can reach — including addresses inside the
    /// deployment that nothing outside it can.
    pub allowed_hosts: BTreeSet<String>,
    /// Whether to insist on `https`. Off only for local development; a
    /// callback over plain HTTP carries the whole document in clear.
    pub require_https: bool,
}

/// Why a token was refused.
///
/// Kept out of what the client is told: [`Refusal::NotAuthorised`] says only
/// that. Which of expired, wrong-audience or wrong-document it was is useful to
/// an operator in a log and useful to an attacker in a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenError {
    /// `exp` has passed.
    Expired,
    /// `nbf` has not arrived.
    NotYetValid,
    /// `aud` is not this server's.
    WrongAudience,
    /// The token is for a different document than the one being joined.
    WrongDocument,
    /// A URL the token names is one this server will not contact.
    ForbiddenUrl {
        /// The URL the token named.
        url: String,
        /// Why the allow-list could not have matched it, when the *configuration*
        /// is the reason rather than the URL.
        ///
        /// `None` unless there is something to say. An operator reading
        /// "forbidden url" about a URL they can see in their own allow-list has
        /// no next step, and the commonest way to get there — writing
        /// `host:port` in `OPENCALC_ALLOWED_HOSTS`, which the comparison strips
        /// the port from and so can never match — is invisible from both ends.
        hint: Option<String>,
    },
    /// A required field was empty.
    Incomplete(&'static str),
}

impl core::fmt::Display for TokenError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TokenError::Expired => f.write_str("the token has expired"),
            TokenError::NotYetValid => f.write_str("the token is not valid yet"),
            TokenError::WrongAudience => f.write_str("the token names a different audience"),
            TokenError::WrongDocument => f.write_str("the token is for a different document"),
            TokenError::ForbiddenUrl { url, hint: None } => {
                write!(f, "the token names a forbidden url: {url}")
            }
            TokenError::ForbiddenUrl {
                url,
                hint: Some(hint),
            } => write!(f, "the token names a forbidden url: {url} — {hint}"),
            TokenError::Incomplete(what) => write!(f, "the token is missing {what}"),
        }
    }
}

impl std::error::Error for TokenError {}

impl TokenError {
    /// What the client is told, which is deliberately less than this.
    #[must_use]
    pub fn refusal(&self) -> Refusal {
        Refusal::NotAuthorised
    }
}

impl Claims {
    /// Check everything decidable without a network.
    ///
    /// The signature is the caller's job — it needs a key, and the key may need
    /// fetching. Everything here is a function of the claims, the policy and a
    /// supplied clock, which is what makes the rare cases testable.
    ///
    /// `document_key` is the session the client asked to join. It is checked
    /// against the token because otherwise a valid token for *any* document
    /// would admit its bearer to *every* document: the signature proves the
    /// host issued it, not that the host issued it for this.
    pub fn validate(
        &self,
        document_key: &str,
        policy: &TokenPolicy,
        now_secs: u64,
    ) -> Result<(), TokenError> {
        if self.exp.saturating_add(policy.leeway_secs) < now_secs {
            return Err(TokenError::Expired);
        }
        if let Some(nbf) = self.nbf
            && nbf > now_secs.saturating_add(policy.leeway_secs)
        {
            return Err(TokenError::NotYetValid);
        }
        if !policy.audience.is_empty() && self.aud != policy.audience {
            return Err(TokenError::WrongAudience);
        }
        if self.document.key != document_key {
            return Err(TokenError::WrongDocument);
        }
        if self.document.key.is_empty() {
            return Err(TokenError::Incomplete("a document key"));
        }
        if self.user.id.is_empty() {
            return Err(TokenError::Incomplete("a user id"));
        }
        check_url(&self.document.url, policy)?;
        if let Some(callback) = &self.callback {
            check_url(callback.endpoint(), policy)?;
        }
        Ok(())
    }

    /// Whether this participant may send `op`.
    #[must_use]
    pub fn permits(&self, op: &Operation) -> bool {
        self.permissions.access.permits(op)
    }
}

/// Whether the server may contact `url`.
fn check_url(url: &str, policy: &TokenPolicy) -> Result<(), TokenError> {
    let forbidden = || TokenError::ForbiddenUrl {
        url: url.to_owned(),
        hint: None,
    };
    let rest = match url.split_once("://") {
        Some(("https", rest)) => rest,
        Some(("http", rest)) if !policy.require_https => rest,
        _ => return Err(forbidden()),
    };
    // Authority ends at the first `/`, `?` or `#`; userinfo is everything up to
    // the last `@`, and is thrown away — a host in the userinfo is the oldest
    // way to make a URL read as one origin and resolve to another.
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default();
    let Some(host) = host_of(authority) else {
        return Err(forbidden());
    };
    if policy.allowed_hosts.is_empty() || allows(policy, host) {
        return Ok(());
    }
    // Refused, and the refusal says so — but if the allow-list itself contains
    // an entry that cannot match anything, say that too. It is the difference
    // between an operator checking their token and an operator fixing their
    // configuration.
    Err(TokenError::ForbiddenUrl {
        url: url.to_owned(),
        hint: names_a_port(policy).map(|entry| {
            format!(
                "the host is compared as {host:?} with the port stripped, and the allowed host \
                 {entry:?} names a port, so it can never match"
            )
        }),
    })
}

/// The host out of a URL authority: no userinfo, no port, no brackets.
///
/// Four shapes, and the third is what `SEC-018` was about: `host`,
/// `host:port`, `[::1]` and `[::1]:8443`. Stripping the port by splitting on
/// the **last** colon is right for the first two and cuts an IPv6 literal in
/// the middle of itself — `[::1]` became `[:`, which trimmed to `":"`, so an
/// `OPENCALC_ALLOWED_HOSTS` entry of `::1` could never match `https://[::1]/`.
/// It failed closed, so it was a dead allow-list entry rather than an open
/// door; it is the same trap as the port one above, and the same cost —
/// configuration that looks correct and matches nothing.
///
/// `None` for an authority this cannot read, which is a refusal: an address
/// the server cannot name is one it should not connect to. So a bracketed
/// literal must contain an IPv6 address and nothing else, an unbracketed host
/// must contain no colon beyond its port, and what follows the brackets can
/// only be a port.
fn host_of(authority: &str) -> Option<&str> {
    let is_port = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    let Some(rest) = authority.strip_prefix('[') else {
        // `host` or `host:port`. A colon inside the host is an IPv6 literal
        // that forgot its brackets, which is not an authority — refused rather
        // than guessed at, because guessing is how this went wrong.
        let host = match authority.split_once(':') {
            Some((host, port)) if is_port(port) => host,
            Some(_) => return None,
            None => authority,
        };
        return (!host.is_empty() && !host.contains([':', '[', ']'])).then_some(host);
    };
    let (inside, after) = rest.split_once(']')?;
    if !after.is_empty() && !after.strip_prefix(':').is_some_and(is_port) {
        return None;
    }
    // Checked as an address, not merely as text between brackets: it is the
    // one thing the brackets are allowed to hold, and parsing it here is what
    // keeps `[anything]` from becoming a host nobody wrote down.
    inside.parse::<std::net::Ipv6Addr>().ok()?;
    Some(inside)
}

/// Whether the allow-list names this host.
///
/// Text first, which is the whole answer for a DNS name. Then as an address,
/// because an IPv6 literal has many spellings of the same thing — `::1`,
/// `0:0:0:0:0:0:0:1`, `[::1]` written with the brackets a URL needs — and an
/// entry that is *the address being refused*, spelled differently, is exactly
/// the failure this function has already produced twice.
fn allows(policy: &TokenPolicy, host: &str) -> bool {
    if policy.allowed_hosts.contains(host) {
        return true;
    }
    let Ok(wanted) = host.parse::<std::net::IpAddr>() else {
        // A name, and names have one spelling here. Comparing them any more
        // loosely — case, trailing dot, unicode — is a widening of a security
        // boundary and belongs to whoever needs it, with its own tests.
        return false;
    };
    policy
        .allowed_hosts
        .iter()
        .any(|entry| unbracketed(entry).parse::<std::net::IpAddr>() == Ok(wanted))
}

/// An allow-list entry without the brackets a URL would put around an IPv6
/// literal, so an operator may write either.
fn unbracketed(entry: &str) -> &str {
    entry
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(entry)
}

/// An allow-list entry that names a port, and therefore matches nothing.
///
/// [`check_url`] compares the host **without** its port, deliberately: a port
/// is not a trust boundary and requiring one would refuse the same host reached
/// on 443 implicitly. The consequence is that `127.0.0.1:8080` in
/// `OPENCALC_ALLOWED_HOSTS` is not a stricter rule, it is a dead one — and
/// nothing said so, which cost real time in a first deployment.
///
/// Only an unambiguous `host:port` counts. A bare IPv6 literal is full of
/// colons and none of them is a port, so `::1` and `2001:db8::1` are left
/// alone — but `[::1]:8443` is a port, and is just as dead.
fn names_a_port(policy: &TokenPolicy) -> Option<&str> {
    policy
        .allowed_hosts
        .iter()
        .find(|entry| {
            let digits = |port: &str| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit());
            if let Some(rest) = entry.strip_prefix('[') {
                // `[literal]:port`. The brackets themselves are fine: an entry
                // may be written with or without them.
                return rest
                    .split_once(']')
                    .is_some_and(|(_, after)| after.strip_prefix(':').is_some_and(digits));
            }
            let Some((host, port)) = entry.split_once(':') else {
                return false;
            };
            !host.is_empty() && digits(port) && !host.contains([':', '[', ']'])
        })
        .map(String::as_str)
}

#[cfg(test)]
mod tests;
