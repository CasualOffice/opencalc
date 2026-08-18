//! The WOPI client: what this service asks of the storage that holds the file.
//!
//! WOPI's naming is the opposite way round from the intuition, and building the
//! wrong side is the classic way to waste a week — the **host** is the
//! *storage* (Nextcloud, SharePoint) and the **client** is the *editor*. This
//! module is the client half: it calls out, it never serves.
//!
//! # The access token is somebody else's credential
//!
//! It is a bearer token for a file store we do not own, and WOPI requires it in
//! the query string, so it is already in the host's access log. It must not
//! also be in ours. Every error out of this module is built by hand from a
//! status code, and transport errors are stripped of their URL with
//! [`reqwest::Error::without_url`] — `reqwest`'s own `Display` prints the URL it
//! failed on, token and all, which is how a credential ends up in a log line
//! nobody thought was about credentials.
//!
//! # Nothing here follows a redirect
//!
//! A redirect would carry that token to whatever host the response named. The
//! same rule the document fetch already follows, for the same reason.

use std::time::Duration;

/// What went wrong talking to the host.
#[derive(Debug)]
pub enum Problem {
    /// The host would not accept the token: expired, or for another file.
    Unauthorised,
    /// Somebody else holds the lock, and this is theirs.
    ///
    /// Carried rather than flattened into a string because it is the one
    /// failure with a recovery: a session that finds its own id here has been
    /// resumed and may carry on, where any other id means a genuine conflict.
    LockMismatch(String),
    /// Anything else, already stripped of anything secret.
    Failed(String),
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Problem::Unauthorised => write!(f, "the host rejected the access token"),
            Problem::LockMismatch(held) => {
                write!(f, "the file is locked by another session ({held})")
            }
            Problem::Failed(why) => write!(f, "{why}"),
        }
    }
}

/// What `CheckFileInfo` tells us about a file.
///
/// A small subset of a large response, and deliberately: every field here is
/// one this service acts on. WOPI hosts return dozens more, and reading fields
/// we do not use invites treating them as guarantees.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct FileInfo {
    /// The filename, extension included. WOPI's only name for the file — the
    /// `WOPISrc` is an opaque id, not something to show a person.
    pub base_file_name: String,
    /// Who is editing, for the presence roster.
    #[serde(default)]
    pub user_friendly_name: Option<String>,
    /// A stable id for that user.
    #[serde(default)]
    pub user_id: Option<String>,
    /// **Whether this user may write.** Defaults to `false`: a host that does
    /// not say must not be assumed permissive, because the failure is silent
    /// and lands on someone else's file.
    #[serde(default)]
    pub user_can_write: bool,
    /// Whether the host implements `Lock`/`Unlock`.
    #[serde(default)]
    pub supports_locks: bool,
    /// Whether the host implements `PutFile`.
    #[serde(default)]
    pub supports_update: bool,
}

/// An HTTP client for one WOPI host.
#[derive(Debug, Clone)]
pub struct Host {
    client: reqwest::Client,
    max_document_bytes: u64,
}

impl Host {
    /// Build a client with the bounds every outbound request needs.
    ///
    /// # Panics
    ///
    /// If the TLS backend cannot be initialised, which is a broken build rather
    /// than a runtime condition.
    #[must_use]
    pub fn new(max_document_bytes: u64) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(10))
                // See the module note: a redirect would take the access token
                // somewhere the host never named.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("a TLS backend"),
            max_document_bytes,
        }
    }

    /// `CheckFileInfo` — what the file is, and what this user may do to it.
    ///
    /// # Errors
    ///
    /// [`Problem::Unauthorised`] if the host rejects the token, which is also
    /// how the token is *validated*: we cannot check someone else's credential
    /// ourselves, so we use it and see.
    pub async fn check_file_info(&self, src: &str, token: &str) -> Result<FileInfo, Problem> {
        let response = self
            .client
            .get(src)
            .query(&[("access_token", token)])
            .send()
            .await
            .map_err(transport)?;
        status(&response)?;
        response.json::<FileInfo>().await.map_err(|e| {
            Problem::Failed(format!(
                "CheckFileInfo was not readable: {}",
                e.without_url()
            ))
        })
    }

    /// `GetFile` — the bytes.
    ///
    /// # Errors
    ///
    /// [`Problem::Failed`] if the file is larger than this service will hold. A
    /// fetch is an untrusted download however trusted the host is: without a
    /// ceiling an endless body makes this process allocate until it dies.
    pub async fn get_file(&self, src: &str, token: &str) -> Result<Vec<u8>, Problem> {
        let response = self
            .client
            .get(contents_of(src))
            .query(&[("access_token", token)])
            .send()
            .await
            .map_err(transport)?;
        status(&response)?;
        if let Some(len) = response.content_length()
            && len > self.max_document_bytes
        {
            return Err(Problem::Failed(format!(
                "the file is {len} bytes, over the {} this service will hold",
                self.max_document_bytes
            )));
        }
        let body = response.bytes().await.map_err(transport)?;
        if body.len() as u64 > self.max_document_bytes {
            return Err(Problem::Failed(format!(
                "the file ran to {} bytes, over the {} this service will hold",
                body.len(),
                self.max_document_bytes
            )));
        }
        Ok(body.to_vec())
    }

    /// `Lock` — take the file, so nothing else writes it while it is open.
    ///
    /// # Errors
    ///
    /// [`Problem::LockMismatch`] with the id already held.
    pub async fn lock(&self, src: &str, token: &str, lock: &str) -> Result<(), Problem> {
        self.override_call(src, token, "LOCK", lock).await
    }

    /// `RefreshLock` — say we are still here.
    ///
    /// # Errors
    ///
    /// As [`Host::lock`].
    pub async fn refresh_lock(&self, src: &str, token: &str, lock: &str) -> Result<(), Problem> {
        self.override_call(src, token, "REFRESH_LOCK", lock).await
    }

    /// `Unlock` — release it.
    ///
    /// # Errors
    ///
    /// As [`Host::lock`].
    pub async fn unlock(&self, src: &str, token: &str, lock: &str) -> Result<(), Problem> {
        self.override_call(src, token, "UNLOCK", lock).await
    }

    /// `PutFile` — save.
    ///
    /// `lock` is the id taken at the start of the session. It is not optional
    /// in practice: SharePoint locks on open, and a `PutFile` without the
    /// matching id is a 409 rather than a save. It is `Option` only because a
    /// host that reports `SupportsLocks: false` has none to send.
    ///
    /// `content_type` is the caller's, not a constant: this service saves back
    /// in whatever format the host's file is in, and a `.csv` announced as an
    /// OOXML package is the same lie as the bytes themselves being wrong —
    /// hosts index, preview and virus-scan on what this header says (`WOPI-05`).
    ///
    /// # Errors
    ///
    /// [`Problem::LockMismatch`] if the lock was lost — which is the case worth
    /// distinguishing, because the bytes are still good and a caller can
    /// re-lock and retry.
    pub async fn put_file(
        &self,
        src: &str,
        token: &str,
        lock: Option<&str>,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<(), Problem> {
        let mut request = self
            .client
            .post(contents_of(src))
            .query(&[("access_token", token)])
            // Without this the host does not know the request is `PutFile`, and
            // answers 404 or 501 rather than saving.
            .header("X-WOPI-Override", "PUT")
            .header(reqwest::header::CONTENT_TYPE, content_type);
        if let Some(lock) = lock {
            request = request.header("X-WOPI-Lock", lock);
        }
        let response = request.body(bytes).send().await.map_err(transport)?;
        status(&response)
    }

    /// The three lock operations differ only by the override header.
    async fn override_call(
        &self,
        src: &str,
        token: &str,
        operation: &str,
        lock: &str,
    ) -> Result<(), Problem> {
        let response = self
            .client
            .post(src)
            .query(&[("access_token", token)])
            .header("X-WOPI-Override", operation)
            .header("X-WOPI-Lock", lock)
            .send()
            .await
            .map_err(transport)?;
        status(&response)
    }
}

/// The contents sub-resource of a `WOPISrc`.
///
/// **`WOPISrc` may carry its own query string**, and appending `/contents` to
/// the whole of it produces `…?a=b/contents` — a URL the host has never heard
/// of, answered with a 404 that reads like a missing file. Hosts that use a
/// path-only src work either way, which is why this is the kind of thing that
/// passes every local test and fails at the first real integration.
fn contents_of(src: &str) -> String {
    let src = src.trim_end_matches('/');
    match src.split_once('?') {
        Some((path, query)) => format!("{path}/contents?{query}"),
        None => format!("{src}/contents"),
    }
}

/// Turn a response status into a [`Problem`], keeping the token out of it.
fn status(response: &reqwest::Response) -> Result<(), Problem> {
    let code = response.status();
    if code.is_success() {
        return Ok(());
    }
    if code == reqwest::StatusCode::UNAUTHORIZED || code == reqwest::StatusCode::FORBIDDEN {
        return Err(Problem::Unauthorised);
    }
    if code == reqwest::StatusCode::CONFLICT {
        // WOPI puts the lock actually held in the header, which is the only way
        // to tell "somebody else has it" from "you lost yours".
        let held = response
            .headers()
            .get("X-WOPI-Lock")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        return Err(Problem::LockMismatch(held));
    }
    Err(Problem::Failed(format!("the host answered {code}")))
}

/// A transport failure, with the URL — and so the token — removed.
fn transport(e: reqwest::Error) -> Problem {
    Problem::Failed(format!("could not reach the host: {}", e.without_url()))
}

#[cfg(test)]
mod tests;
