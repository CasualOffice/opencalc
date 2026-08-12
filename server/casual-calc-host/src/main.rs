//! The reference integrator: what a product embedding OpenCalc has to provide.
//!
//! The collaboration server holds **no per-document state** and deliberately
//! **cannot mint tokens** (ADR-012, ADR-014). It is told, per join, by a party
//! that already knows: where the file lives, where the finished bytes go, who is
//! joining and what they may do. That party is the integrator, and in a product
//! it is the product.
//!
//! Which leaves a gap that mattered more than it looked: there was no way to
//! *see* any of this working without first writing an integrator. This is that
//! integrator, small enough to read in one sitting.
//!
//! # It is a demo, and it is also the guide
//!
//! Two jobs, and they do not conflict. Somebody evaluating OpenCalc runs
//! `docker compose up`, opens a document, presses Share, sends the link, and
//! watches two cursors move. Somebody who has decided to integrate reads this
//! file to find out exactly what their own backend must do — which is four
//! endpoints and a signature.
//!
//! # What it is not
//!
//! Not multi-tenant, not authenticated, and storing documents as files in a
//! directory. Every one of those is a decision a real product makes for itself
//! and would be wrong to inherit from a demo. What it *does* model faithfully is
//! the contract: the token's shape, the fetch, the callback, and the fact that
//! the signing key never leaves this side.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Multipart, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Json, Router};
use casual_calc_model::{Id, Sheet, SheetId, Workbook};
use serde::{Deserialize, Serialize};

/// How this host is configured. Twelve-factor, like the server beside it.
#[derive(Clone)]
struct Config {
    /// Where documents live. A volume in the compose file.
    store: PathBuf,
    /// The secret this host signs tokens with, and the collaboration server
    /// verifies them with. **Shared**, which is why a real product should use
    /// the asymmetric path instead: with a shared secret the server can mint as
    /// well as check, and only the fact that it does not want to stops it.
    secret: String,
    /// What this host is reachable as **from the collaboration server** — a
    /// container name on a compose network, not the address a browser uses.
    /// Getting these two confused is the classic first failure: the server
    /// fetches `localhost` and finds itself.
    internal_base: String,
    /// The WebSocket endpoint a **browser** should connect to.
    collab_ws: String,
    /// The audience the collaboration server is configured to require.
    audience: String,
}

#[derive(Serialize, Deserialize)]
struct DocumentMeta {
    id: String,
    title: String,
}

// --- The token: the whole integration contract ------------------------------

#[derive(Serialize)]
struct User {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<String>,
}

#[derive(Serialize)]
struct Document {
    key: String,
    id: String,
    title: String,
    /// Where the collaboration server fetches the file. **Its** view of this
    /// host, not the browser's.
    url: String,
}

#[derive(Serialize)]
struct Permissions {
    access: &'static str,
}

#[derive(Serialize)]
struct Callback {
    /// Tagged, because a URL alone does not say whether the host wants an
    /// OnlyOffice-style POST or a WOPI `PutFile`.
    kind: &'static str,
    url: String,
}

#[derive(Serialize)]
struct Claims {
    iss: String,
    aud: String,
    exp: u64,
    iat: u64,
    user: User,
    document: Document,
    permissions: Permissions,
    callback: Callback,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Mint a token for `doc`, as `name`, at `access`.
///
/// The one function a real integrator must reimplement, and the reason the
/// signing key lives here: a browser must never be able to make one of these.
fn mint(config: &Config, doc: &DocumentMeta, name: &str, access: &'static str) -> String {
    let issued = now_secs();
    let claims = Claims {
        iss: "opencalc-host".to_owned(),
        aud: config.audience.clone(),
        // Short, because a token is a bearer credential for one document and a
        // share link is forwarded further than people expect.
        exp: issued + 8 * 3600,
        iat: issued,
        user: User {
            // Per browser rather than per person: this host has no accounts, and
            // two tabs must not be one participant or their cursors merge.
            id: format!("u-{}", &uuid()[..8]),
            name: name.to_owned(),
            color: None,
        },
        document: Document {
            // The *session* key. Everyone with the same key joins the same
            // session — which is what a share link is.
            key: doc.id.clone(),
            id: doc.id.clone(),
            title: doc.title.clone(),
            url: format!("{}/api/documents/{}/content", config.internal_base, doc.id),
        },
        permissions: Permissions { access },
        callback: Callback {
            kind: "url",
            url: format!("{}/api/documents/{}/callback", config.internal_base, doc.id),
        },
    };
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(config.secret.as_bytes()),
    )
    .unwrap_or_default()
}

/// Enough uniqueness for a demo, without a dependency.
fn uuid() -> String {
    use std::hash::{BuildHasher as _, RandomState};
    format!(
        "{:016x}{:016x}",
        RandomState::new().hash_one(now_secs()),
        RandomState::new().hash_one("opencalc")
    )
}

// --- Storage ----------------------------------------------------------------

fn doc_path(config: &Config, id: &str) -> Option<PathBuf> {
    // Rejects anything that is not a plain id, which is what stops
    // `../../etc/passwd` being a document name.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    Some(config.store.join(format!("{id}.xlsx")))
}

fn meta_path(config: &Config, id: &str) -> Option<PathBuf> {
    doc_path(config, id).map(|p| p.with_extension("json"))
}

async fn load_meta(config: &Config, id: &str) -> Option<DocumentMeta> {
    let path = meta_path(config, id)?;
    let bytes = tokio::fs::read(path).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// A blank workbook, as `.xlsx` bytes.
fn blank_xlsx(title: &str) -> Vec<u8> {
    let mut workbook = Workbook::new(Id::from_parts(1, 1));
    workbook
        .sheets
        .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "Sheet1"));
    let _ = title;
    casual_calc_export::write_workbook(&workbook).unwrap_or_default()
}

// --- Endpoints --------------------------------------------------------------

#[derive(Deserialize)]
struct NewDoc {
    title: Option<String>,
}

/// Create a document and return where to open it.
async fn create(State(config): State<Arc<Config>>, Json(body): Json<NewDoc>) -> impl IntoResponse {
    let id = uuid()[..16].to_owned();
    let title = body.title.unwrap_or_else(|| "Untitled.xlsx".to_owned());
    let meta = DocumentMeta {
        id: id.clone(),
        title: title.clone(),
    };
    let (Some(doc), Some(metap)) = (doc_path(&config, &id), meta_path(&config, &id)) else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "bad id").into_response();
    };
    if tokio::fs::write(&doc, blank_xlsx(&title)).await.is_err()
        || tokio::fs::write(&metap, serde_json::to_vec(&meta).unwrap_or_default())
            .await
            .is_err()
    {
        return (StatusCode::INTERNAL_SERVER_ERROR, "could not store").into_response();
    }
    Json(serde_json::json!({ "id": id, "open": format!("/d/{id}") })).into_response()
}

/// Upload an existing workbook.
async fn upload(State(config): State<Arc<Config>>, mut form: Multipart) -> impl IntoResponse {
    let mut title = "Uploaded.xlsx".to_owned();
    let mut bytes = Vec::new();
    while let Ok(Some(field)) = form.next_field().await {
        if let Some(name) = field.file_name() {
            title = name.to_owned();
        }
        if let Ok(data) = field.bytes().await {
            bytes = data.to_vec();
        }
    }
    if bytes.is_empty() {
        return (StatusCode::BAD_REQUEST, "no file").into_response();
    }
    let id = uuid()[..16].to_owned();
    let meta = DocumentMeta {
        id: id.clone(),
        title,
    };
    let (Some(doc), Some(metap)) = (doc_path(&config, &id), meta_path(&config, &id)) else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "bad id").into_response();
    };
    let _ = tokio::fs::write(&doc, bytes).await;
    let _ = tokio::fs::write(&metap, serde_json::to_vec(&meta).unwrap_or_default()).await;
    Redirect::to(&format!("/d/{id}")).into_response()
}

/// The bytes, for the collaboration server to fetch.
///
/// Unauthenticated here because this host is a demo on a private network. A real
/// integrator signs this URL or restricts it to the server's identity — the
/// token's `allowed_hosts` is a second line of defence, not the first.
async fn content(State(config): State<Arc<Config>>, Path(id): Path<String>) -> impl IntoResponse {
    let Some(path) = doc_path(&config, &id) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match tokio::fs::read(path).await {
        Ok(bytes) => (
            [(
                header::CONTENT_TYPE,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            )],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// The finished document, sent back by the collaboration server.
///
/// This is the half that makes co-editing durable: the server holds the ordered
/// document while people are in it, and hands the bytes back here when they
/// quiesce. A host that accepts this and drops it has a very convincing demo and
/// no persistence.
async fn callback(
    State(config): State<Arc<Config>>,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let Some(path) = doc_path(&config, &id) else {
        return StatusCode::BAD_REQUEST;
    };
    if body.is_empty() {
        return StatusCode::BAD_REQUEST;
    }
    // Written beside and renamed, so a crash mid-write leaves the previous
    // version rather than half of the new one.
    let tmp = path.with_extension("xlsx.part");
    if tokio::fs::write(&tmp, &body).await.is_err() || tokio::fs::rename(&tmp, &path).await.is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    tracing::info!(%id, bytes = body.len(), "saved");
    StatusCode::OK
}

/// Download whatever has been saved.
async fn download(State(config): State<Arc<Config>>, Path(id): Path<String>) -> impl IntoResponse {
    let (Some(path), Some(meta)) = (doc_path(&config, &id), load_meta(&config, &id).await) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match tokio::fs::read(path).await {
        Ok(bytes) => (
            [
                (
                    header::CONTENT_TYPE,
                    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_owned(),
                ),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", meta.title.replace('"', "")),
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize)]
struct Joining {
    name: Option<String>,
    view: Option<String>,
}

/// What the editor page asks for: a token and where to use it.
async fn session(
    State(config): State<Arc<Config>>,
    Path(id): Path<String>,
    Query(joining): Query<Joining>,
) -> impl IntoResponse {
    let Some(meta) = load_meta(&config, &id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let access = if joining.view.is_some() {
        "view"
    } else {
        "edit"
    };
    let name = joining.name.unwrap_or_else(|| "Guest".to_owned());
    Json(serde_json::json!({
        "token": mint(&config, &meta, &name, access),
        "document": meta.id,
        "title": meta.title,
        "collab": config.collab_ws,
        "editable": access == "edit",
    }))
    .into_response()
}

async fn index() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

async fn open_doc() -> Html<&'static str> {
    Html(include_str!("document.html"))
}

/// Ask the running host whether it is serving, for the container's health check.
///
/// In the binary rather than answered with curl, which the runtime layer does
/// not have — the same reasoning as the collaboration server beside it.
async fn healthy() -> bool {
    let bind = std::env::var("OPENCALC_HOST_BIND").unwrap_or_else(|_| "0.0.0.0:8080".to_owned());
    let port = bind.rsplit(':').next().unwrap_or("8080");
    tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .is_ok()
}

#[tokio::main]
async fn main() {
    if std::env::args().any(|a| a == "--healthcheck") {
        std::process::exit(i32::from(!healthy().await));
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = Arc::new(Config {
        store: std::env::var("OPENCALC_STORE")
            .unwrap_or_else(|_| "/data".to_owned())
            .into(),
        secret: std::env::var("OPENCALC_SHARED_SECRET")
            .unwrap_or_else(|_| "dev-secret-change-me".to_owned()),
        internal_base: std::env::var("OPENCALC_HOST_INTERNAL")
            .unwrap_or_else(|_| "http://host:8080".to_owned()),
        collab_ws: std::env::var("OPENCALC_COLLAB_WS")
            .unwrap_or_else(|_| "ws://127.0.0.1:8443/collab".to_owned()),
        audience: std::env::var("OPENCALC_AUDIENCE").unwrap_or_else(|_| "opencalc-demo".to_owned()),
    });
    if let Err(why) = std::fs::create_dir_all(&config.store) {
        tracing::error!(?why, store = ?config.store, "cannot use the document store");
        std::process::exit(1);
    }

    let editor = std::env::var("OPENCALC_EDITOR_DIR").unwrap_or_else(|_| "/editor".to_owned());
    let app = Router::new()
        .route("/", get(index))
        .route("/d/{id}", get(open_doc))
        .route("/api/documents", post(create))
        .route("/api/upload", post(upload))
        .route("/api/documents/{id}/content", get(content))
        .route("/api/documents/{id}/callback", post(callback))
        .route("/api/documents/{id}/download", get(download))
        .route("/api/documents/{id}/session", get(session))
        .route("/healthz", get(|| async { "ok" }))
        // The editor itself, served from the same origin as the API so a share
        // link is one host and there is no CORS to explain to anybody.
        .nest_service("/editor", tower_http::services::ServeDir::new(editor))
        .with_state(Arc::clone(&config));

    let bind = std::env::var("OPENCALC_HOST_BIND").unwrap_or_else(|_| "0.0.0.0:8080".to_owned());
    let listener = match tokio::net::TcpListener::bind(&bind).await {
        Ok(listener) => listener,
        Err(why) => {
            tracing::error!(%bind, ?why, "cannot bind");
            std::process::exit(1);
        }
    };
    tracing::info!(%bind, store = ?config.store, "opencalc host");
    let _ = axum::serve(listener, app).await;
}
