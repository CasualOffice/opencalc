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
    /// The largest upload accepted, in bytes.
    ///
    /// Axum's default body limit is **2 MB**, which is under the size of an
    /// ordinary spreadsheet and was rejecting real files. Named here rather than
    /// left to a framework default, because the number a deployment wants
    /// depends on its documents and its disk, and neither is knowable from here.
    max_upload: usize,
    /// How many previous versions of a document to keep.
    ///
    /// Bounded because a version is written on every save, and a document being
    /// edited all day would otherwise fill the volume — a demo that eats its own
    /// disk is worse than one with no history.
    max_versions: usize,
    /// The WebSocket endpoint a **browser** should connect to.
    collab_ws: String,
    /// The audience the collaboration server is configured to require.
    audience: String,
    /// Enables the admin page when set. Absent means there is no admin page,
    /// which is the right default for something that ships as a demo.
    admin_token: Option<String>,
}

/// Settings an operator may change **while it is running**.
///
/// # What is here, and what is deliberately not
///
/// Everything in this struct can change under a live server without anybody
/// losing work: the next browser to ask for a session gets the new value.
///
/// What is **not** here is the rest of the configuration — bind addresses, TLS,
/// the signing secret, the Redis URL, a node's identity — and that is a decision
/// rather than an omission. Those cannot change beneath an open connection: a
/// new secret invalidates every token in flight, a new bind address is a
/// different server, and a node changing identity mid-lease is the zombie the
/// epoch exists to fence. They stay environment-only, where changing one means
/// restarting the thing it configures, which is the honest cost.
///
/// Stored beside the documents so it survives a restart, which is the "apart
/// from env or a config file" part: the file *is* written here, by the admin
/// page, rather than being something an operator edits and then restarts for.
#[derive(Clone, Serialize, Deserialize)]
struct Settings {
    /// What the browser is told to connect to. Changing it moves every *new*
    /// session; the ones already open keep the socket they have.
    collab_ws: String,
    /// Whether the landing page accepts uploads.
    allow_uploads: bool,
    /// Shown on the landing page, so a demo can be labelled for who it is for.
    banner: String,
}

impl Settings {
    fn path(config: &Config) -> PathBuf {
        config.store.join("settings.json")
    }

    /// The stored settings, or the ones the environment implies.
    fn load(config: &Config) -> Self {
        std::fs::read(Self::path(config))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_else(|| Self {
                collab_ws: config.collab_ws.clone(),
                allow_uploads: true,
                banner: String::new(),
            })
    }

    fn save(&self, config: &Config) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).unwrap_or_default();
        std::fs::write(Self::path(config), bytes)
    }
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

/// Where a document's previous versions live.
///
/// A directory beside the document, one file per version, named for the moment
/// it was taken. **The name is the metadata**: no second file to keep in step
/// with the first, nothing to go stale, and a directory listing is already in
/// order.
fn versions_dir(config: &Config, id: &str) -> Option<PathBuf> {
    doc_path(config, id).map(|p| p.with_extension("versions"))
}

/// The path of one version, refusing anything that is not a timestamp.
///
/// The version id arrives in a URL, so it is chosen by whoever wrote the link —
/// the same reason `doc_path` refuses anything that is not a plain id. Digits
/// only means `..` cannot appear at all.
fn version_path(config: &Config, id: &str, at: &str) -> Option<PathBuf> {
    if at.is_empty() || !at.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    versions_dir(config, id).map(|d| d.join(format!("{at}.xlsx")))
}

/// Milliseconds since the Unix epoch, as a version name.
fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// Every version of `id`, newest first.
async fn list_versions(config: &Config, id: &str) -> Vec<(u128, u64)> {
    let Some(dir) = versions_dir(config, id) else {
        return Vec::new();
    };
    let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
        // No directory means no versions, which is the ordinary case for a
        // document nobody has saved twice — not an error.
        return Vec::new();
    };
    let mut out = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let Some(stem) = name.to_str().and_then(|n| n.strip_suffix(".xlsx")) else {
            continue;
        };
        let Ok(at) = stem.parse::<u128>() else {
            continue;
        };
        let bytes = entry.metadata().await.map(|m| m.len()).unwrap_or(0);
        out.push((at, bytes));
    }
    // Newest first: a list of saves is read from the top.
    out.sort_unstable_by_key(|(at, _)| std::cmp::Reverse(*at));
    out
}

/// Keep what the document says *now* as a version, before something replaces it.
///
/// Called before every write that would lose the current bytes — a save and a
/// restore alike. A restore that did not do this would be the one destructive
/// button in the product with no way back, which is precisely what version
/// history exists to prevent.
///
/// Failures are logged and swallowed on purpose. Not being able to keep a
/// version is worse if it also refuses the save that prompted it: the save is
/// the user's work, the version is a convenience.
async fn keep_version(config: &Config, id: &str) {
    let (Some(doc), Some(dir)) = (doc_path(config, id), versions_dir(config, id)) else {
        return;
    };
    let Ok(current) = tokio::fs::read(&doc).await else {
        return; // Nothing to keep: no document yet.
    };
    if current.is_empty() {
        return;
    }
    if tokio::fs::create_dir_all(&dir).await.is_err() {
        tracing::warn!(%id, "could not create the version directory");
        return;
    }
    // **Two saves in the same millisecond must not become one version.**
    //
    // The timestamp is the name, so a collision silently overwrites the older
    // of the two and history quietly loses an entry — the failure that is
    // hardest to notice, because the list still looks plausible. A save that
    // lands in an occupied millisecond takes the next free one instead; the
    // bound stops a pathological clock spinning here for ever.
    let mut at = now_millis();
    let mut path = dir.join(format!("{at}.xlsx"));
    for _ in 0..1000 {
        if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
            break;
        }
        at += 1;
        path = dir.join(format!("{at}.xlsx"));
    }
    if let Err(why) = tokio::fs::write(&path, &current).await {
        tracing::warn!(%id, ?why, "could not keep a version");
        return;
    }
    prune_versions(config, id).await;
}

/// Drop the oldest versions beyond `max_versions`.
async fn prune_versions(config: &Config, id: &str) {
    let Some(dir) = versions_dir(config, id) else {
        return;
    };
    for (at, _) in list_versions(config, id)
        .await
        .into_iter()
        .skip(config.max_versions)
    {
        let _ = tokio::fs::remove_file(dir.join(format!("{at}.xlsx"))).await;
    }
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
    // The admin page offers this switch, so it has to mean something. A setting
    // that is accepted and ignored is the failure `/admin` documents itself as
    // refusing to commit.
    if !Settings::load(&config).allow_uploads {
        return (StatusCode::FORBIDDEN, "uploads are turned off").into_response();
    }

    let mut title = "Uploaded.xlsx".to_owned();
    let mut bytes = Vec::new();
    loop {
        match form.next_field().await {
            Ok(Some(field)) => {
                if let Some(name) = field.file_name() {
                    title = name.to_owned();
                }
                match field.bytes().await {
                    Ok(data) => bytes = data.to_vec(),
                    // Distinguished rather than swallowed. Every failure here
                    // used to leave `bytes` empty and answer "no file", so a
                    // spreadsheet one byte over the limit was reported as no
                    // spreadsheet at all — which sends somebody looking at their
                    // file picker instead of at the limit.
                    Err(why) => {
                        tracing::warn!(%title, ?why, "upload body rejected");
                        return (
                            StatusCode::PAYLOAD_TOO_LARGE,
                            format!(
                                "could not read the upload (limit is {} MB)",
                                config.max_upload / (1024 * 1024)
                            ),
                        )
                            .into_response();
                    }
                }
            }
            Ok(None) => break,
            Err(why) => {
                tracing::warn!(?why, "malformed upload");
                return (StatusCode::BAD_REQUEST, "malformed upload").into_response();
            }
        }
    }
    if bytes.is_empty() {
        return (StatusCode::BAD_REQUEST, "no file").into_response();
    }

    // Admitted before it is stored, not after.
    //
    // Storing first and discovering on open produces a document that exists,
    // has a share link, and cannot be opened by anybody it was shared with —
    // and the person who uploaded it finds out one navigation later, with no
    // way to tell a corrupt file from a broken server. The importer is the only
    // thing that actually knows, so it is what decides.
    if let Err(why) = casual_calc_import::import_package(bytes.clone()) {
        tracing::info!(%title, ?why, "upload refused");
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            format!("this file could not be opened as a spreadsheet: {why}"),
        )
            .into_response();
    }

    let id = uuid()[..16].to_owned();
    let meta = DocumentMeta {
        id: id.clone(),
        title,
    };
    let (Some(doc), Some(metap)) = (doc_path(&config, &id), meta_path(&config, &id)) else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "bad id").into_response();
    };
    // Both writes checked, and the document written first. A metadata file with
    // no document behind it is a listing entry that opens to nothing; ignoring
    // either error produced exactly that, silently, on a full disk.
    if let Err(why) = tokio::fs::write(&doc, bytes).await {
        tracing::error!(?why, ?doc, "cannot store the uploaded document");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not store the file",
        )
            .into_response();
    }
    if let Err(why) = tokio::fs::write(&metap, serde_json::to_vec(&meta).unwrap_or_default()).await
    {
        tracing::error!(?why, ?metap, "cannot store the document metadata");
        // The orphan is removed rather than left behind, so a retry is a clean
        // upload instead of a second copy beside an unreachable first.
        let _ = tokio::fs::remove_file(&doc).await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not store the file",
        )
            .into_response();
    }
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
    // **Before the overwrite**, so a version is what the document *was* rather
    // than what it became — which is what somebody reaching for history wants
    // (`COL-41`).
    keep_version(&config, &id).await;
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

/// Every kept version of a document, newest first.
///
/// Sizes rather than a preview: a list is for choosing, and a person choosing
/// between saves recognises "the one from before lunch" by its time. Rendering
/// each version to say what changed would mean opening every one of them to
/// draw a list.
async fn versions(State(config): State<Arc<Config>>, Path(id): Path<String>) -> impl IntoResponse {
    if load_meta(&config, &id).await.is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let list: Vec<_> = list_versions(&config, &id)
        .await
        .into_iter()
        .map(|(at, bytes)| serde_json::json!({ "at": at.to_string(), "bytes": bytes }))
        .collect();
    Json(serde_json::json!({ "versions": list })).into_response()
}

/// One version's bytes, so it can be looked at before it is restored.
///
/// Downloading a version is not restoring it, and having both is what makes
/// restore a considered act rather than a guess — somebody can open the old
/// file, check it is the one they meant, and only then replace what is live.
async fn version_download(
    State(config): State<Arc<Config>>,
    Path((id, at)): Path<(String, String)>,
) -> impl IntoResponse {
    let (Some(path), Some(meta)) = (
        version_path(&config, &id, &at),
        load_meta(&config, &id).await,
    ) else {
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
                    // Prefixed with the version, so downloading one does not
                    // land on top of the live file in somebody's downloads
                    // folder — which would make "look at it before restoring"
                    // the thing that loses their current work.
                    format!(
                        "attachment; filename=\"{at}-{}\"",
                        meta.title.replace('"', "")
                    ),
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Make a kept version the live document again.
///
/// **The current document becomes a version first**, so a restore is itself
/// undoable. Without that, the one button in the product whose whole purpose is
/// undoing a mistake would be the only one that cannot be undone.
///
/// The version is left in place rather than moved, so restoring the same one
/// twice does the same thing both times.
async fn version_restore(
    State(config): State<Arc<Config>>,
    Path((id, at)): Path<(String, String)>,
) -> impl IntoResponse {
    let (Some(from), Some(doc)) = (version_path(&config, &id, &at), doc_path(&config, &id)) else {
        return (StatusCode::BAD_REQUEST, "not a version id").into_response();
    };
    if load_meta(&config, &id).await.is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(bytes) = tokio::fs::read(&from).await else {
        return (StatusCode::NOT_FOUND, "no such version").into_response();
    };
    // Refuse to restore something that cannot be opened, rather than making it
    // live and finding out when somebody tries. The importer is the only thing
    // that actually knows — the same check the upload path makes.
    if let Err(why) = casual_calc_import::import_package(bytes.clone()) {
        tracing::warn!(%id, %at, ?why, "refused to restore an unopenable version");
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "that version cannot be opened as a spreadsheet",
        )
            .into_response();
    }

    keep_version(&config, &id).await;
    let tmp = doc.with_extension("xlsx.part");
    if tokio::fs::write(&tmp, &bytes).await.is_err() || tokio::fs::rename(&tmp, &doc).await.is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    tracing::info!(%id, %at, bytes = bytes.len(), "restored");
    Json(serde_json::json!({ "restored": at, "bytes": bytes.len() })).into_response()
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

/// Where the *browser* should open its collaboration socket.
///
/// An explicit `OPENCALC_COLLAB_WS` always wins: a deployment that puts the
/// collaboration server on its own hostname has to be able to say so.
///
/// Unset, it is **derived from the request the browser just made**, which is
/// the only address known to be reachable from where the browser is. The
/// previous default was `ws://127.0.0.1:8443/collab` — the *browser's* own
/// loopback, not the server's. That works only when the browser runs on the
/// Docker host: a second participant on another machine dialled themselves and
/// reconnected forever, and an HTTPS page could not open `ws://` at all
/// (PROD-12). It made the demo unusable for its entire purpose, which is
/// sending somebody a link.
///
/// Echoing `Host` back is not an escalation: the browser reached this handler
/// through that name, so it is a name the browser can resolve. `X-Forwarded-Proto`
/// decides `ws` against `wss`, and a spoofed one costs a failed connection
/// rather than a leak — the alternative, guessing the scheme, breaks every TLS
/// deployment.
/// Give a configured endpoint the `/collab` path if it has none.
///
/// The derived form below always ends in `/collab`, because that is the only
/// path the collaboration server serves. A **configured** endpoint used to be
/// returned exactly as written — so `OPENCALC_COLLAB_WS=ws://collab:8443`,
/// which is what the variable's own name suggests, produced an address nothing
/// answers. The socket failed, the editor stayed blank, and there was nothing
/// in any log on either side to say why: the host had done as it was told and
/// the server was never asked.
///
/// Only the clearly-wrong case is repaired. An endpoint with a real path is
/// left alone, because a deployment behind a proxy may genuinely mount the
/// socket somewhere else, and silently rewriting *that* would break the setup
/// it was configured for.
fn with_collab_path(configured: &str) -> String {
    let trimmed = configured.trim_end_matches('/');
    // Past the scheme, so the `//` in `ws://` is not mistaken for the path.
    let authority_at = trimmed.find("://").map_or(0, |i| i + 3);
    if trimmed[authority_at..].contains('/') {
        return configured.to_owned();
    }
    format!("{trimmed}/collab")
}

fn collab_endpoint(config: &Config, headers: &axum::http::HeaderMap) -> String {
    let configured = Settings::load(config).collab_ws;
    if !configured.is_empty() {
        return with_collab_path(&configured);
    }
    let secure = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|proto| proto.split(',').next().unwrap_or("").trim() == "https");
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|v| v.to_str().ok())
        .map(|h| h.split(',').next().unwrap_or(h).trim())
        .filter(|h| !h.is_empty())
        // No Host header at all is HTTP/1.0 or a hand-written request; the
        // browser will not be one, so this is a floor rather than a case.
        .unwrap_or("127.0.0.1:8080");
    let scheme = if secure { "wss" } else { "ws" };
    format!("{scheme}://{host}/collab")
}

/// What the editor page asks for: a token and where to use it.
async fn session(
    State(config): State<Arc<Config>>,
    Path(id): Path<String>,
    Query(joining): Query<Joining>,
    headers: axum::http::HeaderMap,
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
        "collab": collab_endpoint(&config, &headers),
        "editable": access == "edit",
    }))
    .into_response()
}

/// Whether a request may administer this host.
///
/// Admin is **off unless a token is set**, rather than on with a default. A demo
/// that ships with a known password is a demo somebody exposes to the internet
/// once and then explains, and "it was only a demo" is not a thing anybody says
/// afterwards.
fn admin_ok(config: &Config, headers: &axum::http::HeaderMap) -> bool {
    let Some(expected) = config.admin_token.as_deref() else {
        return false;
    };
    headers
        .get("x-admin-token")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|got| got == expected)
}

/// Everything an operator can see: what is running, what is stored, and every
/// effective setting — including the ones they cannot change here, and why.
async fn admin_state(
    State(config): State<Arc<Config>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if !admin_ok(&config, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let settings = Settings::load(&config);

    let mut documents = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&config.store) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && path.file_stem().is_some_and(|s| s != "settings")
                && let Ok(bytes) = std::fs::read(&path)
                && let Ok(meta) = serde_json::from_slice::<DocumentMeta>(&bytes)
            {
                let size = std::fs::metadata(path.with_extension("xlsx"))
                    .map(|m| m.len())
                    .unwrap_or(0);
                documents.push(serde_json::json!({
                    "id": meta.id, "title": meta.title, "bytes": size,
                }));
            }
        }
    }

    Json(serde_json::json!({
        "runtime": settings,
        "fixed": {
            "audience": config.audience,
            "store": config.store.to_string_lossy(),
            "host_internal": config.internal_base,
            "secret_set": !config.secret.is_empty(),
            // Named on the admin page because "why is this document boxes?"
            // is otherwise unanswerable from inside the product.
            "font_dir": font_dir().to_string_lossy(),
            "fonts": faces_in(&font_dir()),
        },
        "documents": documents,
    }))
    .into_response()
}

/// Where a deployment drops the faces it needs.
fn font_dir() -> std::path::PathBuf {
    std::env::var("OPENCALC_FONT_DIR")
        .unwrap_or_else(|_| "/fonts".to_owned())
        .into()
}

/// The face files in `dir`, sorted so the editor registers them in the same
/// order on every boot — coverage is decided by *first* match, so an unstable
/// order would mean the same document rendering in different faces.
fn faces_in(dir: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| {
            let lower = n.to_ascii_lowercase();
            [".ttf", ".otf", ".ttc"]
                .iter()
                .any(|ext| lower.ends_with(ext))
        })
        .collect();
    names.sort();
    names
}

/// What the editor asks for at boot, so it can register each one.
///
/// **URLs, not names.** The editor fetching what it is told to fetch is the
/// difference between a host being free to serve faces from anywhere — a CDN, a
/// versioned path, another origin — and every client having to reconstruct a
/// convention that only this host happens to follow.
async fn font_list() -> impl IntoResponse {
    let urls: Vec<String> = faces_in(&font_dir())
        .into_iter()
        .map(|name| format!("/fonts/{name}"))
        .collect();
    Json(serde_json::json!({ "fonts": urls }))
}

/// Change what can be changed.
async fn admin_update(
    State(config): State<Arc<Config>>,
    headers: axum::http::HeaderMap,
    Json(next): Json<Settings>,
) -> impl IntoResponse {
    if !admin_ok(&config, &headers) {
        return StatusCode::UNAUTHORIZED;
    }
    // Refused rather than accepted-and-ignored: a setting that silently does
    // not apply is worse than one that cannot be set, because the operator
    // believes it took.
    if next.collab_ws.is_empty() {
        return StatusCode::BAD_REQUEST;
    }
    match next.save(&config) {
        Ok(()) => {
            tracing::info!(collab_ws = %next.collab_ws, "settings changed");
            StatusCode::OK
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn admin_page() -> Html<&'static str> {
    Html(include_str!("admin.html"))
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
        max_versions: std::env::var("OPENCALC_MAX_VERSIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(20),
        max_upload: std::env::var("OPENCALC_MAX_UPLOAD_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(64 * 1024 * 1024),
        // Empty means "derive it from the request" — see `collab_endpoint`.
        // A default of `ws://127.0.0.1:8443/collab` looked like a working
        // configuration and was one only for a browser on the Docker host.
        collab_ws: std::env::var("OPENCALC_COLLAB_WS").unwrap_or_default(),
        audience: std::env::var("OPENCALC_AUDIENCE").unwrap_or_else(|_| "opencalc-demo".to_owned()),
        admin_token: std::env::var("OPENCALC_ADMIN_TOKEN")
            .ok()
            .filter(|t| !t.is_empty()),
    });
    if let Err(why) = std::fs::create_dir_all(&config.store) {
        tracing::error!(?why, store = ?config.store, "cannot use the document store");
        std::process::exit(1);
    }

    let editor = std::env::var("OPENCALC_EDITOR_DIR").unwrap_or_else(|_| "/editor".to_owned());
    // Fonts the deployment supplies. Empty is the normal case and stays silent;
    // this exists so that supplying one is dropping a file into a directory
    // rather than a code change, which is the whole premise of ADR-018's
    // "a host knows which scripts its documents are in".
    let fonts = font_dir();
    match faces_in(&fonts) {
        faces if faces.is_empty() => tracing::info!(
            dir = ?fonts,
            "no fonts supplied; Latin renders, other scripts will not (see docs/65)"
        ),
        faces => tracing::info!(dir = ?fonts, count = faces.len(), "fonts supplied"),
    }
    let app = Router::new()
        .route("/", get(index))
        .route("/d/{id}", get(open_doc))
        .route("/api/documents", post(create))
        .route(
            "/api/upload",
            post(upload).layer(axum::extract::DefaultBodyLimit::max(config.max_upload)),
        )
        .route("/api/documents/{id}/content", get(content))
        .route("/api/documents/{id}/callback", post(callback))
        .route("/api/documents/{id}/download", get(download))
        // History lives on the host because storage does. The collaboration
        // server is deliberately not a file store (docs/57), and a WOPI
        // deployment already has its own versioning — this is for the SDK and
        // demo paths, which had none (`COL-41`).
        .route("/api/documents/{id}/versions", get(versions))
        .route("/api/documents/{id}/versions/{at}", get(version_download))
        .route(
            "/api/documents/{id}/versions/{at}/restore",
            post(version_restore),
        )
        .route("/api/documents/{id}/session", get(session))
        .route("/healthz", get(|| async { "ok" }))
        .route("/admin", get(admin_page))
        .route("/api/admin/state", get(admin_state))
        .route("/api/admin/settings", post(admin_update))
        // The editor itself, served from the same origin as the API so a share
        // link is one host and there is no CORS to explain to anybody.
        .route("/api/fonts", get(font_list))
        .nest_service("/editor", tower_http::services::ServeDir::new(editor))
        // Served rather than embedded, and from the same origin as everything
        // else so registering one needs no CORS.
        .nest_service("/fonts", tower_http::services::ServeDir::new(fonts))
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

#[cfg(test)]
mod endpoint_tests {
    use super::*;
    use axum::http::HeaderMap;

    /// **A configured endpoint with no path still reaches the socket.**
    ///
    /// The collaboration server serves `/collab` and nothing else, and the
    /// derived endpoint has always ended in it. A configured one was returned
    /// verbatim, so `OPENCALC_COLLAB_WS=ws://collab:8443` — which is exactly
    /// what the variable's name invites — produced an address nothing answers.
    ///
    /// It is a silent failure on both sides: the host did as it was told, the
    /// server was never asked, and the only symptom is an editor that never
    /// connects. Found by running the stack rather than reading it (`PROD-13`).
    #[test]
    fn a_configured_endpoint_without_a_path_gets_the_collab_one() {
        // **Through `collab_endpoint`, not the helper.** Asserting on
        // `with_collab_path` alone passes with the call site reverted — the
        // helper is still correct, it is just no longer used. That is the
        // defect this test exists for, so it has to go through the function a
        // session actually calls.
        let at = |configured: &str| collab_endpoint(&config(configured), &headers(&[]));
        assert_eq!(at("ws://collab:8443"), "ws://collab:8443/collab");
        assert_eq!(at("wss://calc.example"), "wss://calc.example/collab");
        // A trailing slash is the same mistake with a slash on the end.
        assert_eq!(at("ws://collab:8443/"), "ws://collab:8443/collab");
    }

    /// **An endpoint that names a path keeps it.**
    ///
    /// A deployment behind a proxy may mount the socket anywhere, and silently
    /// rewriting a deliberate path would break the setup it was configured for
    /// — turning a fix for one mistake into a different one.
    #[test]
    fn a_configured_endpoint_with_a_path_is_left_alone() {
        let at = |configured: &str| collab_endpoint(&config(configured), &headers(&[]));
        assert_eq!(at("wss://calc.example/ws"), "wss://calc.example/ws");
        assert_eq!(at("wss://calc.example/collab"), "wss://calc.example/collab");
        assert_eq!(
            at("wss://calc.example/edit/socket"),
            "wss://calc.example/edit/socket"
        );
    }

    fn config(collab_ws: &str) -> Config {
        Config {
            max_versions: 20,
            store: PathBuf::from("/tmp/opencalc-test-store"),
            secret: "s".into(),
            internal_base: "http://host:8080".into(),
            max_upload: 1024,
            collab_ws: collab_ws.to_owned(),
            audience: "a".into(),
            admin_token: None,
        }
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        h
    }

    /// PROD-12. The address handed to the browser must be one the *browser*
    /// can reach.
    ///
    /// It used to be `ws://127.0.0.1:8443/collab` — loopback, and a different
    /// port from the page. That is reachable only from the Docker host, so the
    /// second participant a share link exists for dialled themselves and
    /// reconnected forever. The address the browser already used to get here is
    /// the one address known to work.
    #[test]
    fn the_endpoint_follows_the_host_the_browser_actually_used() {
        let at = collab_endpoint(&config(""), &headers(&[("host", "sheets.example.com")]));
        assert_eq!(at, "ws://sheets.example.com/collab");
    }

    /// An HTTPS page cannot open `ws://` at all, so guessing the scheme breaks
    /// every TLS deployment rather than merely inconveniencing it.
    #[test]
    fn tls_termination_upstream_produces_a_wss_endpoint() {
        let at = collab_endpoint(
            &config(""),
            &headers(&[
                ("host", "sheets.example.com"),
                ("x-forwarded-proto", "https"),
            ]),
        );
        assert_eq!(at, "wss://sheets.example.com/collab");
    }

    /// A chain of proxies appends rather than replaces, and the first entry is
    /// the one the browser spoke to.
    #[test]
    fn only_the_first_hop_of_a_forwarded_chain_is_used() {
        let at = collab_endpoint(
            &config(""),
            &headers(&[
                ("host", "internal:8080"),
                ("x-forwarded-host", "sheets.example.com, internal:8080"),
                ("x-forwarded-proto", "https, http"),
            ]),
        );
        assert_eq!(at, "wss://sheets.example.com/collab");
    }

    /// A deployment that puts collaboration on its own hostname has to be able
    /// to say so, and saying so must beat any derivation.
    #[test]
    fn an_explicit_endpoint_always_wins() {
        let at = collab_endpoint(
            &config("wss://collab.example.com/collab"),
            &headers(&[("host", "sheets.example.com")]),
        );
        assert_eq!(at, "wss://collab.example.com/collab");
    }
}

#[cfg(test)]
mod version_tests {
    use super::*;

    /// A store of its own per test, so retention and pruning cannot see each
    /// other's files.
    fn store(name: &str) -> Arc<Config> {
        let dir =
            std::env::temp_dir().join(format!("opencalc-versions-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Arc::new(Config {
            store: dir,
            secret: "s".into(),
            internal_base: "http://host:8080".into(),
            max_upload: 1 << 20,
            max_versions: 3,
            collab_ws: String::new(),
            audience: "a".into(),
            admin_token: None,
        })
    }

    async fn put(config: &Config, id: &str, bytes: &[u8]) {
        tokio::fs::write(doc_path(config, id).unwrap(), bytes)
            .await
            .unwrap();
        tokio::fs::write(
            meta_path(config, id).unwrap(),
            serde_json::to_vec(&DocumentMeta {
                id: id.to_owned(),
                title: "T.xlsx".into(),
            })
            .unwrap(),
        )
        .await
        .unwrap();
    }

    /// **A save keeps what the document was, not what it became.**
    ///
    /// Somebody reaching for history wants the state they are trying to get
    /// back to. A version taken *after* the write would be a copy of the thing
    /// they are trying to undo, and the list would look right while being
    /// useless.
    #[tokio::test]
    async fn a_save_keeps_the_previous_contents() {
        let config = store("previous");
        put(&config, "doc", b"first").await;

        keep_version(&config, "doc").await;
        tokio::fs::write(doc_path(&config, "doc").unwrap(), b"second")
            .await
            .unwrap();

        let versions = list_versions(&config, "doc").await;
        assert_eq!(versions.len(), 1, "no version was kept");
        let at = versions[0].0.to_string();
        let kept = tokio::fs::read(version_path(&config, "doc", &at).unwrap())
            .await
            .unwrap();
        assert_eq!(
            kept, b"first",
            "the version holds the new bytes, not the old ones"
        );
    }

    /// **Newest first**, because a list of saves is read from the top.
    #[tokio::test]
    async fn versions_are_listed_newest_first() {
        let config = store("order");
        put(&config, "doc", b"one").await;
        for body in [b"two".as_slice(), b"three", b"four"] {
            keep_version(&config, "doc").await;
            tokio::fs::write(doc_path(&config, "doc").unwrap(), body)
                .await
                .unwrap();
        }
        let versions = list_versions(&config, "doc").await;
        assert_eq!(versions.len(), 3);
        let times: Vec<u128> = versions.iter().map(|v| v.0).collect();
        let mut sorted = times.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(times, sorted, "versions came back oldest first: {times:?}");
    }

    /// **Two versions in the same millisecond are both kept.**
    ///
    /// The name is the timestamp, so a collision overwrites the older one and
    /// history loses an entry while still looking plausible — the failure that
    /// is hardest to notice. Saves this fast are exactly what a script or a
    /// burst of collaborative activity produces.
    #[tokio::test]
    async fn two_versions_in_the_same_millisecond_are_both_kept() {
        let config = store("collide");
        put(&config, "doc", b"a").await;
        // No sleeps: the point is that the code does not need them.
        keep_version(&config, "doc").await;
        tokio::fs::write(doc_path(&config, "doc").unwrap(), b"b")
            .await
            .unwrap();
        keep_version(&config, "doc").await;

        assert_eq!(
            list_versions(&config, "doc").await.len(),
            2,
            "one of two versions taken in the same millisecond was lost"
        );
    }

    /// **Retention is bounded**, and drops the oldest.
    #[tokio::test]
    async fn only_the_most_recent_versions_are_kept() {
        let config = store("prune");
        put(&config, "doc", b"v0").await;
        for n in 1..8 {
            keep_version(&config, "doc").await;
            tokio::fs::write(doc_path(&config, "doc").unwrap(), format!("v{n}"))
                .await
                .unwrap();
        }
        let versions = list_versions(&config, "doc").await;
        assert_eq!(
            versions.len(),
            config.max_versions,
            "retention did not bound the list"
        );

        // The survivors are the newest ones, not an arbitrary three.
        let newest = versions[0].0;
        let oldest_kept = versions[versions.len() - 1].0;
        assert!(newest >= oldest_kept);
        let bodies: Vec<Vec<u8>> = {
            let mut out = Vec::new();
            for (at, _) in &versions {
                out.push(
                    tokio::fs::read(version_path(&config, "doc", &at.to_string()).unwrap())
                        .await
                        .unwrap(),
                );
            }
            out
        };
        assert!(
            bodies.iter().all(|b| b != b"v0"),
            "the oldest version survived pruning: {bodies:?}"
        );
    }

    /// **Zero keeps none**, which `.env.example` offers as a way to turn the
    /// feature off.
    ///
    /// Gated because it is a documented contract, and a documented contract
    /// nothing checks is the kind that quietly stops being true. Here it also
    /// happens to be the boundary case of the pruning arithmetic.
    #[tokio::test]
    async fn a_maximum_of_zero_keeps_no_versions() {
        let mut config = store("none");
        Arc::get_mut(&mut config).unwrap().max_versions = 0;
        put(&config, "doc", b"one").await;

        keep_version(&config, "doc").await;
        keep_version(&config, "doc").await;

        assert!(
            list_versions(&config, "doc").await.is_empty(),
            "versions were kept with the maximum set to zero"
        );
    }

    /// **Restoring keeps the current document first**, so a restore is itself
    /// undoable.
    ///
    /// Without this, the one button whose whole purpose is undoing a mistake
    /// would be the only one that cannot be undone.
    #[tokio::test]
    async fn restoring_is_itself_undoable() {
        let config = store("restore");
        let good = crate::blank_xlsx("T.xlsx");
        put(&config, "doc", &good).await;
        keep_version(&config, "doc").await;
        let target = list_versions(&config, "doc").await[0].0.to_string();

        // The document moves on.
        tokio::fs::write(doc_path(&config, "doc").unwrap(), &good)
            .await
            .unwrap();

        let before = list_versions(&config, "doc").await.len();
        let response = version_restore(
            State(Arc::clone(&config)),
            axum::extract::Path(("doc".to_owned(), target)),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK, "the restore was refused");

        assert_eq!(
            list_versions(&config, "doc").await.len(),
            before + 1,
            "restoring did not keep what it replaced"
        );
    }

    /// **A save through the real handler keeps the previous contents.**
    ///
    /// Through `callback`, not through `keep_version`. The ordering — version
    /// first, write second — lives in the caller, so a test that calls the
    /// helper directly passes with the two swapped and the whole point lost:
    /// every version would be a copy of the save that replaced it.
    #[tokio::test]
    async fn a_save_through_the_handler_keeps_what_it_replaced() {
        let config = store("callback");
        // **Distinct bytes, not two blank workbooks.** `blank_xlsx` ignores its
        // title, so two of them are byte-identical and the assertion below
        // cannot tell "kept the old one" from "kept the new one" — this test
        // passed with the ordering swapped until that was noticed. Neither
        // `callback` nor `keep_version` parses what it is handed, so plain
        // bytes exercise the same path.
        let first = b"FIRST-CONTENTS".to_vec();
        put(&config, "doc", &first).await;
        let second = b"SECOND-CONTENTS".to_vec();
        let response = callback(
            State(Arc::clone(&config)),
            axum::extract::Path("doc".to_owned()),
            axum::body::Bytes::from(second.clone()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let versions = list_versions(&config, "doc").await;
        assert_eq!(versions.len(), 1, "the save kept no version");
        let kept =
            tokio::fs::read(version_path(&config, "doc", &versions[0].0.to_string()).unwrap())
                .await
                .unwrap();
        assert_eq!(
            kept, first,
            "the version holds what the save wrote, not what it replaced"
        );
    }

    /// **A version id that is not a timestamp is refused.**
    ///
    /// It arrives in a URL, so it is chosen by whoever wrote the link. Digits
    /// only means `..` cannot appear at all — the same guard `doc_path` makes,
    /// for the same reason.
    #[tokio::test]
    async fn a_version_id_cannot_escape_the_store() {
        let config = store("escape");
        for hostile in ["..", "../../etc/passwd", "1/../..", "abc", ""] {
            assert!(
                version_path(&config, "doc", hostile).is_none(),
                "{hostile:?} was accepted as a version id"
            );
        }
        assert!(version_path(&config, "doc", "1700000000000").is_some());
    }

    /// **A version that cannot be opened is not made live.**
    ///
    /// Restoring first and discovering later leaves a document that exists, is
    /// shared, and opens for nobody — with no way to tell a corrupt file from a
    /// broken server. The importer is the only thing that knows, which is the
    /// same argument the upload path makes.
    #[tokio::test]
    async fn an_unopenable_version_is_refused_rather_than_restored() {
        let config = store("corrupt");
        let good = crate::blank_xlsx("T.xlsx");
        put(&config, "doc", &good).await;

        let dir = versions_dir(&config, "doc").unwrap();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("1700000000000.xlsx"), b"not a spreadsheet")
            .await
            .unwrap();

        let response = version_restore(
            State(Arc::clone(&config)),
            axum::extract::Path(("doc".to_owned(), "1700000000000".to_owned())),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let live = tokio::fs::read(doc_path(&config, "doc").unwrap())
            .await
            .unwrap();
        assert_eq!(
            live, good,
            "the live document was replaced with rubbish anyway"
        );
    }
}
