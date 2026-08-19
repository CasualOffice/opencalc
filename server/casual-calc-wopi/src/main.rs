//! The WOPI adapter: a WOPI **client** on one side, an ordinary OpenCalc
//! integrator on the other.
//!
//! WOPI is how an editor becomes installable. Nextcloud, ownCloud, SharePoint,
//! Moodle and Alfresco do not each have an integration API — they have this
//! one, and Collabora Online and ONLYOFFICE are installed into all five by
//! implementing it. An administrator pastes one URL into a settings page and
//! OpenCalc appears in the list of editors.
//!
//! The design is [docs/74](../../../docs/74-WOPI-INTEGRATION.md). The short
//! version of why this is its own service:
//!
//! - The collaboration server **cannot mint tokens and holds no per-document
//!   state** (ADR-012, ADR-014). WOPI needs both. Putting them there would undo
//!   the property that makes that server safe to scale.
//! - `casual-calc-host` is the demo integrator and says so in its own first
//!   paragraph. An integrator inheriting a demo is what that file warns against.
//!
//! So this is a third service, and the collaboration server needed no changes
//! at all to gain a WOPI integration — it is handed the same signed claims any
//! integrator produces.
//!
//! # Both legs are proxied, deliberately
//!
//! The server fetches the document from *us* and posts the finished bytes to
//! *us*, rather than being pointed at the WOPI host directly. The host's access
//! token is a bearer credential for somebody else's file store; keeping it here
//! keeps it out of the server's configuration, its logs and its cluster log.
//! One extra hop, three fewer places to leak from.
//!
//! # And that is where the format conversion lives
//!
//! The collaboration server reads and writes OOXML packages, and only those: a
//! document has one canonical form whatever opened it. A WOPI host holds
//! whatever its user uploaded. Because both legs already pass through this
//! process, this is the one place that can hold both truths at once — the
//! *package* goes to the server, the *original format* goes back to the host,
//! and neither end ever learns about the other.
//!
//! Which is what lets [`mod@discovery`] advertise more than `xlsx` (`WOPI-05`).
//! Everything a format cannot carry is counted and named by `describe_loss`
//! before the bytes leave: a `.csv` is one sheet of values, and an administrator
//! finding that out from the file rather than from the log is the failure this
//! row was opened for.

mod config;
mod discovery;
mod proof;
mod sessions;
mod token;
mod wopi;

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use casual_calc_sdk::{SessionFormat, WorkbookSession};

use config::Config;
use sessions::{Session, Sessions, fresh_id};
use wopi::{Host, Problem};

/// How often a held lock is refreshed. WOPI locks expire after 30 minutes.
const REFRESH_EVERY_MS: u64 = 10 * 60 * 1000;
/// How often the sweeper runs.
const SWEEP_EVERY_MS: u64 = 60 * 1000;

/// Everything the handlers share.
struct Service {
    config: Config,
    host: Host,
    sessions: Sessions,
    /// Absent unless a key is configured — see `Config::proof_key_path`.
    proof: Option<std::sync::Arc<proof::ProofKeys>>,
}

/// What a WOPI host puts on the action URL.
#[derive(Debug, serde::Deserialize)]
struct Opening {
    /// The file's WOPI endpoint. WOPI spells it exactly this way.
    #[serde(rename = "WOPISrc")]
    src: String,
    /// The host's access token for this user and this file.
    access_token: String,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

/// The discovery document an administrator points their host at.
async fn discovery(State(service): State<Arc<Service>>) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
        discovery::document(
            &service.config.public_url,
            &service.config.brand,
            service.proof.as_deref(),
        ),
    )
}

/// Open a file for editing.
async fn edit(
    State(service): State<Arc<Service>>,
    Query(opening): Query<Opening>,
) -> axum::response::Response {
    open(&service, opening, true).await
}

/// Open a file read-only, whatever the host says this user may do.
async fn view(
    State(service): State<Arc<Service>>,
    Query(opening): Query<Opening>,
) -> axum::response::Response {
    open(&service, opening, false).await
}

/// The action URL: check the file, take a lock, and serve the editor.
async fn open(
    service: &Arc<Service>,
    opening: Opening,
    writable: bool,
) -> axum::response::Response {
    // Before anything is fetched. The `WOPISrc` arrives in a query string, so
    // it is chosen by whoever wrote the link the user clicked.
    if let Err(why) = service.config.permits(&opening.src) {
        tracing::warn!("refused a WOPISrc: {why}");
        return (StatusCode::BAD_REQUEST, why).into_response();
    }

    let info = match service
        .host
        .check_file_info(&opening.src, &opening.access_token)
        .await
    {
        Ok(info) => info,
        Err(Problem::Unauthorised) => {
            // The host rejected the token, which is also how it is validated —
            // we hold no key that could check somebody else's credential.
            return (
                StatusCode::UNAUTHORIZED,
                "the host did not accept this access token",
            )
                .into_response();
        }
        Err(why) => {
            tracing::error!("CheckFileInfo failed: {why}");
            return (
                StatusCode::BAD_GATEWAY,
                "the host could not be asked about this file",
            )
                .into_response();
        }
    };

    // **Before the lock**, because a file this service cannot write back is one
    // it must not hold open. Discovery advertises the formats `format_for`
    // knows, so a host following it never lands here; a hand-written link, a
    // stale host configuration or an `.ods` does, and the alternative to
    // refusing is opening it and saving a package over it under its own name.
    let Some(format) = sessions::format_for(&info.base_file_name) else {
        tracing::warn!("refused a file this service cannot save back in its own format");
        return (
            StatusCode::BAD_REQUEST,
            "this editor cannot save that kind of file back in its own format",
        )
            .into_response();
    };

    let now = now_ms();
    let mut session = Session::from(
        opening.src.clone(),
        opening.access_token.clone(),
        &info,
        format,
        now,
    );
    // A view action is read-only regardless of what the user is entitled to.
    session.editable = session.editable && writable;

    let wants_lock = session.editable && info.supports_locks;
    let lock = wants_lock.then(|| fresh_id(16));
    if let Some(lock) = &lock
        && let Err(why) = service
            .host
            .lock(&opening.src, &opening.access_token, lock)
            .await
    {
        // Somebody else is editing. Opening read-only is strictly better than
        // refusing: the file is readable, and an editor that says "read-only,
        // someone else has it" is a better answer than an error page.
        tracing::info!("could not lock, opening read-only: {why}");
        session.editable = false;
    }
    let lock = session.editable.then_some(lock).flatten();

    let id = match service.sessions.insert(session, now) {
        Ok(id) => id,
        Err(orphan) => {
            // The lock was taken a moment ago and this session will never
            // exist, so nothing else would ever release it.
            if let Some(lock) = &lock {
                let _ = service.host.unlock(&orphan.src, &orphan.token, lock).await;
            }
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "this node is holding as many documents as it will",
            )
                .into_response();
        }
    };
    service.sessions.set_lock(&id, lock, now);

    // The id is 32 bytes of hex, so this is the one interpolation into markup
    // and it cannot carry markup.
    Html(include_str!("editor.html").replace("__SESSION_ID__", &id)).into_response()
}

/// What the page needs to start the editor.
async fn session_info(
    State(service): State<Arc<Service>>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let now = now_ms();
    let Some(session) = service.sessions.get(&id, now) else {
        return (StatusCode::NOT_FOUND, "no such session").into_response();
    };
    let brand = &service.config.brand;
    Json(serde_json::json!({
        "token": token::mint(&service.config, &session, &id, now / 1000),
        "collab": service.config.collab_url,
        // The brand travels on the iframe's own URL. The editor is a static
        // bundle served from somewhere else entirely — often a CDN — so the
        // only thing this service can configure about it is its address.
        "editor": branded(&service.config.editor_url, brand),
        "title": session.title,
        "editable": session.editable,
        // Said at the top of the session, not at the end of it. A `.csv` keeps
        // one sheet of values, and the moment to learn that is before an hour
        // of work goes into a second sheet.
        "format": session.format.extension(),
        "brand": brand.name,
        "accent": brand.accent,
        "favicon": brand.favicon,
    }))
    .into_response()
}

/// The editor's URL, carrying the brand.
///
/// Appended with `&` when the URL already has a query — the editor's own
/// `?fonts=` is the common case, and a second `?` makes the whole query one
/// literal parameter name, so the fonts stop working and the brand never
/// arrives.
fn branded(editor_url: &str, brand: &discovery::Brand) -> String {
    if *brand == discovery::Brand::default() {
        return editor_url.to_owned();
    }
    let join = if editor_url.contains('?') { '&' } else { '?' };
    let mut url = format!("{editor_url}{join}brand={}", encode(&brand.name));
    if !brand.accent.is_empty() {
        url.push_str(&format!("&accent={}", encode(&brand.accent)));
    }
    url
}

/// Percent-encode everything that is not unreserved.
///
/// A brand name is operator text going into a query string: a `&` in it would
/// otherwise start a parameter, and a `#` would truncate the URL.
fn encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The document's bytes, fetched from the host on the server's behalf.
async fn content(
    State(service): State<Arc<Service>>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let Some(session) = service.sessions.get(&id, now_ms()) else {
        return (StatusCode::NOT_FOUND, "no such session").into_response();
    };
    let bytes = match service.host.get_file(&session.src, &session.token).await {
        Ok(bytes) => bytes,
        Err(why) => {
            tracing::error!("GetFile failed: {why}");
            return (StatusCode::BAD_GATEWAY, "the host would not serve the file").into_response();
        }
    };
    // Whatever the host holds, the collaboration server is handed a package —
    // so the content type below is true of every answer this endpoint gives,
    // which it was not before.
    let package = match to_package(bytes, session.format) {
        Ok((package, loss)) => {
            if let Some(loss) = loss {
                // **The way in is a conversion too.** A `.ods` carries styles,
                // merges and charts this engine's reader does not model, and
                // they are gone from the package the editor works on — so they
                // are gone from what is written back. Said here, at the moment
                // the document is admitted, because that is when an
                // administrator can still tell somebody to keep the original.
                tracing::warn!(
                    "reading a .{} drops what this engine does not model: {loss}",
                    session.format.extension()
                );
            }
            package
        }
        Err(why) => {
            tracing::error!("the host's file could not be read: {why}");
            return (
                StatusCode::BAD_GATEWAY,
                "the file could not be read as the kind of file it is named",
            )
                .into_response();
        }
    };
    (
        [(
            header::CONTENT_TYPE,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        )],
        package,
    )
        .into_response()
}

/// The host's bytes, in the OOXML package the collaboration server reads — and
/// what reading them cost.
///
/// An `.xlsx` is handed straight through — untouched, not re-imported and
/// re-written. That matters beyond the wasted work: a round trip through the
/// semantic writer would rebuild every part this engine does not model, so
/// merely *opening* a workbook and closing it again would change the file even
/// when nobody typed anything.
///
/// Every other format is *read*, and a reader that does not model everything in
/// the file loses it here rather than on the way out. Returned as a pair for the
/// same reason [`save_as`] is: the caller decides what to do with the loss, and
/// a function that only returned bytes would make the silence the default.
fn to_package(bytes: Vec<u8>, format: SessionFormat) -> Result<(Vec<u8>, Option<String>), String> {
    if format == SessionFormat::Xlsx {
        return Ok((bytes, None));
    }
    let session =
        WorkbookSession::open_as(bytes, format).map_err(|e| format!("could not open: {e}"))?;
    let loss = describe_loss(session.compatibility_report());
    let package = session
        .save_as(SessionFormat::Xlsx)
        .map_err(|e| format!("could not convert to a package: {e}"))?;
    Ok((package, loss))
}

/// The finished package, in the format the file on the host is in — and what
/// that format could not carry.
///
/// The loss is returned rather than logged here so the caller decides what to
/// do with it. It is never `None` when something was dropped: that is the whole
/// contract, and the reason this returns a pair instead of just bytes.
fn save_as(package: Vec<u8>, format: SessionFormat) -> Result<(Vec<u8>, Option<String>), String> {
    if format == SessionFormat::Xlsx {
        return Ok((package, None));
    }
    let session = WorkbookSession::open(package)
        .map_err(|e| format!("the finished package could not be read back: {e}"))?;
    let loss = describe_loss(&session.loss_writing(format));
    let bytes = session
        .save_as(format)
        .map_err(|e| format!("could not write the file back in its own format: {e}"))?;
    Ok((bytes, loss))
}

/// A compatibility report as one line, or `None` when it is empty.
///
/// Every feature is **named and counted**. A summary that said "some formatting
/// was lost" would be the same silence in a longer sentence: the administrator
/// reading this log line is deciding whether to tell somebody their file needs
/// re-saving as `.xlsx`, and "3 sheets" and "1 merged cell" are different
/// answers to that.
fn describe_loss(report: &casual_calc_sdk::CompatibilityReport) -> Option<String> {
    let named: Vec<String> = report
        .entries()
        .into_iter()
        .map(|e| format!("{} ({})", e.feature, e.count))
        .collect();
    (!named.is_empty()).then(|| named.join(", "))
}

/// The finished package, on its way back to the host.
async fn callback(
    State(service): State<Arc<Service>>,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let Some(session) = service.sessions.get(&id, now_ms()) else {
        return (StatusCode::NOT_FOUND, "no such session").into_response();
    };
    if !session.editable {
        // A read-only session has no lock and no business writing. Refusing
        // here rather than trusting the server keeps the decision in the place
        // that asked the host about it.
        return (StatusCode::FORBIDDEN, "this session is read-only").into_response();
    }

    // Back into the format the host's file is in, before a byte is sent. A
    // failure here is **not** a save: answering anything successful would tell
    // the collaboration server the work is safe when nothing was written.
    let (bytes, loss) = match save_as(body.to_vec(), session.format) {
        Ok(converted) => converted,
        Err(why) => {
            tracing::error!("could not convert the finished document: {why}");
            return (StatusCode::BAD_GATEWAY, why).into_response();
        }
    };
    if let Some(loss) = loss {
        // Named and counted, not "some formatting was lost". The save goes
        // ahead — the user is editing a `.csv` and asked for a `.csv` — but no
        // part of a document leaves this process without being said out loud.
        tracing::warn!(
            "saving as .{} drops what that format cannot hold: {loss}",
            session.format.extension()
        );
    }

    match service
        .host
        .put_file(
            &session.src,
            &session.token,
            session.lock.as_deref(),
            session.format.content_type(),
            bytes,
        )
        .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(why) => {
            // **Never a success.** The collaboration server treats a failed
            // callback as unsaved work and keeps the document; answering 200
            // here is how edits disappear silently.
            tracing::error!("PutFile failed: {why}");
            (StatusCode::BAD_GATEWAY, why.to_string()).into_response()
        }
    }
}

/// The editor closed. Release the lock now rather than at expiry.
async fn close(State(service): State<Arc<Service>>, Path(id): Path<String>) -> StatusCode {
    if let Some(session) = service.sessions.remove(&id)
        && let Some(lock) = &session.lock
        && let Err(why) = service
            .host
            .unlock(&session.src, &session.token, lock)
            .await
    {
        tracing::warn!("could not release a lock on close: {why}");
    }
    StatusCode::NO_CONTENT
}

/// Refresh the locks that are due, and release the ones whose session has gone.
async fn sweep(service: &Arc<Service>) {
    let now = now_ms();
    for (id, session) in service.sessions.due_for_refresh(now, REFRESH_EVERY_MS) {
        let Some(lock) = session.lock.clone() else {
            continue;
        };
        match service
            .host
            .refresh_lock(&session.src, &session.token, &lock)
            .await
        {
            Ok(()) => service.sessions.set_lock(&id, Some(lock), now),
            Err(why) => {
                // The lock is gone and will not come back by waiting. The
                // session keeps its bytes and will fail its next save loudly,
                // which is the honest outcome.
                tracing::warn!("lost a lock: {why}");
            }
        }
    }
    for session in service.sessions.take_expired(now) {
        if let Some(lock) = &session.lock {
            let _ = service
                .host
                .unlock(&session.src, &session.token, lock)
                .await;
        }
    }
}

fn router(service: Arc<Service>) -> Router {
    Router::new()
        .route("/hosting/discovery", get(discovery))
        .route("/wopi/edit", get(edit))
        .route("/wopi/view", get(view))
        .route("/wopi/session/{id}", get(session_info))
        .route("/wopi/content/{id}", get(content))
        .route("/wopi/callback/{id}", post(callback))
        .route("/wopi/close/{id}", post(close))
        // Liveness is unconditional; readiness says whether this node will take
        // another document. The same split the collaboration server makes.
        .route("/healthz", get(|| async { "ok\n" }))
        .route(
            "/readyz",
            get(|State(service): State<Arc<Service>>| async move {
                if service.sessions.len() >= service.config.max_sessions {
                    (StatusCode::SERVICE_UNAVAILABLE, "not ready: at capacity\n")
                } else {
                    (StatusCode::OK, "ready\n")
                }
            }),
        )
        .with_state(service)
}

/// Ask the running service whether it is serving, for the container's health
/// check — the image has no curl, the same as the two services beside it.
async fn probe(path: &str) -> Result<(), String> {
    let bind = std::env::var("OPENCALC_WOPI_BIND").unwrap_or_else(|_| "0.0.0.0:8090".to_owned());
    let port = bind.rsplit(':').next().unwrap_or("8090");
    let response = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}{path}"))
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
        .map_err(|e| format!("could not reach {port}: {}", e.without_url()))?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("{path} answered {}", response.status()))
    }
}

async fn start() -> Result<(), String> {
    let config = Config::from_env()?;
    tracing::info!(
        "{} serving WOPI at {}/hosting/discovery for {} host(s)",
        config.brand.name,
        config.public_url,
        config.allowed_hosts.len()
    );
    if config.allow_plain {
        tracing::warn!(
            "OPENCALC_WOPI_ALLOW_PLAIN is set: an access token will travel in clear. \
             Local development only."
        );
    }

    // **A configured key that will not load is fatal, not a warning.** An
    // operator who set `OPENCALC_WOPI_PROOF_KEY` wants requests signed; starting
    // anyway would advertise no proof key and sign nothing, and the deployment
    // would look healthy while being exactly as unprotected as before.
    let proof = match &config.proof_key_path {
        Some(path) => {
            let der = std::fs::read(path)
                .map_err(|e| format!("OPENCALC_WOPI_PROOF_KEY {}: {e}", path.display()))?;
            let keys = proof::ProofKeys::from_pkcs8(&der)
                .map_err(|e| format!("OPENCALC_WOPI_PROOF_KEY {}: {e}", path.display()))?;
            tracing::info!("WOPI proof keys enabled");
            Some(keys)
        }
        None => None,
    };

    let proof = proof.map(std::sync::Arc::new);
    let service = Arc::new(Service {
        host: Host::new(config.max_document_bytes, proof.clone()),
        sessions: Sessions::new(config.max_sessions, config.session_ttl_ms),
        config,
        proof,
    });

    let sweeper = Arc::clone(&service);
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(SWEEP_EVERY_MS));
        loop {
            tick.tick().await;
            sweep(&sweeper).await;
        }
    });

    let listener = tokio::net::TcpListener::bind(&service.config.bind)
        .await
        .map_err(|e| format!("could not bind {}: {e}", service.config.bind))?;
    axum::serve(listener, router(Arc::clone(&service)))
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            // Every open session holds a lock on somebody's file. Leaving them
            // held means every document this node had open is locked against
            // its owner until WOPI's own 30-minute expiry.
            tracing::info!("releasing {} lock(s)", service.sessions.len());
            for session in service.sessions.take_expired(u64::MAX) {
                if let Some(lock) = &session.lock {
                    let _ = service
                        .host
                        .unlock(&session.src, &session.token, lock)
                        .await;
                }
            }
        })
        .await
        .map_err(|e| e.to_string())
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "casual_calc_wopi=info,warn".into()),
        )
        .init();

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
            tracing::error!("{why}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests;
