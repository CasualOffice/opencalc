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

mod config;
mod discovery;
mod sessions;
mod token;
mod wopi;

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};

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
        discovery::document(&service.config.public_url, &service.config.brand),
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

    let now = now_ms();
    let mut session = Session::from(
        opening.src.clone(),
        opening.access_token.clone(),
        &info,
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
    match service.host.get_file(&session.src, &session.token).await {
        Ok(bytes) => (
            [(
                header::CONTENT_TYPE,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            )],
            bytes,
        )
            .into_response(),
        Err(why) => {
            tracing::error!("GetFile failed: {why}");
            (StatusCode::BAD_GATEWAY, "the host would not serve the file").into_response()
        }
    }
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

    match service
        .host
        .put_file(
            &session.src,
            &session.token,
            session.lock.as_deref(),
            body.to_vec(),
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

    let service = Arc::new(Service {
        host: Host::new(config.max_document_bytes),
        sessions: Sessions::new(config.max_sessions, config.session_ttl_ms),
        config,
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
