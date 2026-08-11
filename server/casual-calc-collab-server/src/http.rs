//! Talking to the integrator: fetching a document and handing it back.
//!
//! The implementations of [`Fetch`] and [`Deliver`] that a deployment actually
//! runs. They are separate from the traits, and the traits exist, because an
//! integrator may need mutual TLS, a proxy or a signed request that this crate
//! should not have opinions about — and because a test must be able to observe
//! a save without a network.
//!
//! # Both callback shapes, because they are different requests
//!
//! [`Callback::Url`] is OnlyOffice's shape: **POST** the package to one URL.
//! [`Callback::Wopi`] is a WOPI host: **PUT** to `{src}/contents` bearing the
//! access token, with the `X-WOPI-Override: PUT` header that tells a WOPI
//! server this is `PutFile` and not something else. Guessing which from the
//! shape of a string is how that goes wrong, which is why the token tags it.
//!
//! # Trust comes from the operating system, not from a bundled list
//!
//! The client uses the **system trust store** rather than a compiled-in copy of
//! Mozilla's roots. That is not a licensing convenience; it is the difference
//! between working and not working in the deployments this is for. An
//! integrator inside a company is routinely behind a certificate issued by that
//! company's own CA, and a bundled public root list rejects it — correctly, and
//! uselessly. The system store is where an operator has already put the roots
//! they trust, including their own.
//!
//! The cost is a deployment requirement: the image needs a CA bundle
//! (`ca-certificates`), where a bundled list would have needed nothing. That is
//! the right way round — a missing CA bundle fails loudly at the first request,
//! where an internal CA that cannot be added fails permanently.
//!
//! # What is bounded here
//!
//! A request to somebody else's server is the classic place for a service to
//! wait forever. Every request has a timeout, a response has a size ceiling,
//! and redirects are **not** followed — a redirect would take the request
//! somewhere the token's allow-list never approved, which is the whole point of
//! having one.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::net::{Deliver, Fetch};
use crate::token::Callback;

/// The MIME type a spreadsheet package is sent as.
const XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

/// An HTTP client for the integrator's endpoints.
#[derive(Debug, Clone)]
pub struct HttpTransport {
    client: reqwest::Client,
    /// The largest document this will accept from an origin.
    ///
    /// A fetch is an untrusted download: without a ceiling, an origin that
    /// serves an endless body makes the node allocate until it dies, and it
    /// need not even be hostile to do it.
    max_document_bytes: u64,
}

/// How the transport behaves.
#[derive(Debug, Clone, Copy)]
pub struct HttpConfig {
    /// How long to wait for the integrator, per request.
    pub timeout: Duration,
    /// The largest document accepted from an origin.
    pub max_document_bytes: u64,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            // Long enough for a large document over a slow link, short enough
            // that a wedged origin does not pin a task for minutes.
            timeout: Duration::from_secs(30),
            max_document_bytes: 256 * 1024 * 1024,
        }
    }
}

impl HttpTransport {
    /// Build a client.
    ///
    /// # Errors
    ///
    /// If the underlying client cannot be constructed — in practice, a TLS
    /// backend that will not initialise.
    pub fn new(config: HttpConfig) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            // Not followed on purpose: a redirect would take this request
            // somewhere the token's allow-list never approved, and that list is
            // the only thing standing between a mis-issued token and every
            // address inside the deployment.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            client,
            max_document_bytes: config.max_document_bytes,
        })
    }
}

impl Default for HttpTransport {
    fn default() -> Self {
        Self::new(HttpConfig::default()).expect("the default client builds")
    }
}

impl Fetch for HttpTransport {
    fn get(&self, url: String) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send>> {
        let client = self.client.clone();
        let ceiling = self.max_document_bytes;
        Box::pin(async move {
            let response = client.get(&url).send().await.map_err(|e| e.to_string())?;
            let status = response.status();
            if !status.is_success() {
                return Err(format!("the origin answered {status}"));
            }
            // Checked before reading where the origin declares a length, and
            // again while reading where it does not — a missing or lying
            // `Content-Length` is exactly what an endless body has.
            if let Some(len) = response.content_length()
                && len > ceiling
            {
                return Err(format!(
                    "the document is {len} bytes, over the {ceiling} ceiling"
                ));
            }
            let mut body = Vec::new();
            let mut response = response;
            while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
                if body.len() as u64 + chunk.len() as u64 > ceiling {
                    return Err(format!("the document exceeded the {ceiling} byte ceiling"));
                }
                body.extend_from_slice(&chunk);
            }
            Ok(body)
        })
    }
}

impl Deliver for HttpTransport {
    fn put(
        &self,
        destination: Callback,
        title: String,
        bytes: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> {
        let client = self.client.clone();
        Box::pin(async move {
            let request = match &destination {
                Callback::Url { url } => client
                    .post(url)
                    .header(reqwest::header::CONTENT_TYPE, XLSX)
                    // So a host that serves many documents can tell which
                    // arrived without parsing the package.
                    .header("X-OpenCalc-Title", sanitise(&title)),
                Callback::Wopi { src, token, .. } => {
                    let url = format!("{}/contents", src.trim_end_matches('/'));
                    client
                        .put(&url)
                        .query(&[("access_token", token.as_str())])
                        // Without this a WOPI server does not know the request
                        // is `PutFile`, and answers 404 or 501 rather than
                        // saving.
                        .header("X-WOPI-Override", "PUT")
                        .header(reqwest::header::CONTENT_TYPE, XLSX)
                }
            };

            let response = request
                .body(bytes)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let status = response.status();
            if status.is_success() {
                return Ok(());
            }
            // The body may say something useful and may also be a whole HTML
            // error page, so it is bounded before it reaches a log line.
            let detail = response.text().await.unwrap_or_default();
            let detail: String = detail.chars().take(200).collect();
            Err(format!("the host answered {status}: {detail}"))
        })
    }
}

/// Strip anything that cannot go in a header value.
///
/// A document title comes from the token, which comes from the integrator — but
/// a filename with a newline in it would let a title split a header and inject
/// another, and a title is exactly the field most likely to hold whatever a
/// user typed.
fn sanitise(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .take(200)
        .collect()
}

#[cfg(test)]
mod tests;
