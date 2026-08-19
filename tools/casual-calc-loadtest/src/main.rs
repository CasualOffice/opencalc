//! What a node actually costs under people, rather than in a benchmark.
//!
//! `PERF-10` measures the engine: 83.64 bytes a cell for a workbook built in
//! process. `DOC-030` turns that into a sizing table. Neither is a *node* under
//! load, and the gap between them is where a capacity claim earns its keep —
//! the operation log, participant state, the broadcast path and the socket
//! buffers all sit on top of the per-cell figure and none of them were measured
//! (`PERF-12`).
//!
//! This opens `--documents` documents with `--clients` participants in each,
//! edits at a steady rate, and reports what the node's own `/metrics` says
//! about its resident size alongside the latency its clients saw.
//!
//! # Why it reads the server's metrics rather than its own memory
//!
//! The harness and the server are different processes, and it is the *server*
//! that has to hold the documents. Measuring the harness would measure the
//! harness. `SRV-03` exposes `opencalc_resident_bytes` for exactly this, which
//! also means an operator watching a live node reads the same number this does.
//!
//! # What it does not do
//!
//! It does not assert a budget. There is no calibrated figure to assert yet —
//! this is the tool that produces one, and a threshold invented before the
//! first measurement is the "uncalibrated gate" `PERF-10` warned about.

use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;

/// What one run found.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    documents: usize,
    clients_per_document: usize,
    edits_sent: u64,
    edits_acknowledged: u64,
    /// Latency from submitting an edit to the server acknowledging it.
    p50_micros: u64,
    p99_micros: u64,
    max_micros: u64,
    /// The node's resident size before any client connected.
    baseline_resident_bytes: Option<u64>,
    /// And at the end, with everything open.
    peak_resident_bytes: Option<u64>,
    /// What those documents cost the node, per document.
    bytes_per_document: Option<u64>,
}

fn arg(args: &[String], name: &str) -> Option<String> {
    let at = args.iter().position(|a| a == name)?;
    args.get(at + 1).cloned()
}

fn number(args: &[String], name: &str, fallback: usize) -> usize {
    arg(args, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(fallback)
}

/// Pull one gauge out of a Prometheus exposition body.
///
/// A tiny parser rather than a dependency: the format is one metric a line and
/// this needs exactly one of them. Separated so it can be tested without a
/// server, which is the only way it gets tested at all on a developer machine.
fn gauge(body: &str, name: &str) -> Option<u64> {
    for line in body.lines() {
        if line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix(name)
            && let Some(value) = rest.split_whitespace().next()
        {
            return value.parse().ok();
        }
    }
    None
}

/// The percentile of a sorted slice, by nearest rank.
///
/// Nearest rank rather than interpolation, because a latency figure that is not
/// a latency anybody experienced is hard to reason about — p99 should be a
/// measurement, not an average of two.
fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let base = arg(&args, "--url").unwrap_or_else(|| "http://127.0.0.1:8443".to_owned());
    let secret = arg(&args, "--secret").unwrap_or_else(|| {
        std::env::var("OPENCALC_SHARED_SECRET").unwrap_or_else(|_| "loadtest".to_owned())
    });
    let audience = arg(&args, "--audience").unwrap_or_else(|| "opencalc-demo".to_owned());
    let origin = arg(&args, "--origin").unwrap_or_else(|| "http://127.0.0.1:8080".to_owned());
    let documents = number(&args, "--documents", 10);
    let clients = number(&args, "--clients", 3);
    let seconds = number(&args, "--seconds", 10);

    let http = reqwest::Client::new();
    let read_resident = |http: reqwest::Client, base: String| async move {
        let body = http
            .get(format!("{base}/metrics"))
            .send()
            .await
            .ok()?
            .text()
            .await
            .ok()?;
        gauge(&body, "opencalc_resident_bytes")
    };

    let baseline = read_resident(http.clone(), base.clone()).await;

    let ws_base = base.replacen("http", "ws", 1);
    let mut tasks = Vec::new();
    for doc in 0..documents {
        for client in 0..clients {
            let (ws_base, secret, audience, origin) = (
                ws_base.clone(),
                secret.clone(),
                audience.clone(),
                origin.clone(),
            );
            tasks.push(tokio::spawn(async move {
                run_client(&ws_base, &secret, &audience, &origin, doc, client, seconds).await
            }));
        }
    }

    let mut latencies: Vec<u64> = Vec::new();
    let mut sent = 0u64;
    for task in tasks {
        if let Ok(Ok((client_sent, mut client_latencies))) = task.await {
            sent += client_sent;
            latencies.append(&mut client_latencies);
        }
    }
    latencies.sort_unstable();

    let peak = read_resident(http, base).await;
    let bytes_per_document = match (baseline, peak) {
        (Some(base), Some(peak)) if documents > 0 && peak > base => {
            Some((peak - base) / documents as u64)
        }
        _ => None,
    };

    let report = Report {
        documents,
        clients_per_document: clients,
        edits_sent: sent,
        edits_acknowledged: latencies.len() as u64,
        p50_micros: percentile(&latencies, 50.0),
        p99_micros: percentile(&latencies, 99.0),
        max_micros: latencies.last().copied().unwrap_or(0),
        baseline_resident_bytes: baseline,
        peak_resident_bytes: peak,
        bytes_per_document,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);

    // A load test that measured nothing must not report success. See `verdict`.
    verdict(report.edits_sent, report.edits_acknowledged)?;

    Ok(())
}

/// One participant: join, then edit at a steady rate for `seconds`.
async fn run_client(
    ws_base: &str,
    secret: &str,
    audience: &str,
    origin: &str,
    doc: usize,
    client: usize,
    seconds: usize,
) -> Result<(u64, Vec<u64>), Box<dyn std::error::Error + Send + Sync>> {
    let key = format!("load-{doc}");
    let token = mint(secret, audience, origin, &key, client)?;
    let url = format!("{ws_base}/collab?doc={key}");
    let (mut socket, _) = tokio_tungstenite::connect_async(&url).await?;

    let protocol = casual_calc_transaction::protocol::PROTOCOL_VERSION;
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({ "type": "join", "protocol": protocol, "token": token }).to_string(),
        ))
        .await?;

    // Wait for the welcome before editing: a submission before the document is
    // open is refused, and counting those as latency would measure the refusal.
    let mut me = 0u64;
    let mut revision = 0u64;
    while let Some(Ok(message)) = socket.next().await {
        let tokio_tungstenite::tungstenite::Message::Text(text) = message else {
            continue;
        };
        let value: serde_json::Value = serde_json::from_str(&text)?;
        if value["type"] == "welcome" {
            me = value["client"].as_u64().unwrap_or(0);
            revision = value["revision"].as_u64().unwrap_or(0);
            break;
        }
    }

    let deadline = Instant::now() + Duration::from_secs(seconds as u64);
    let mut sent = 0u64;
    let mut latencies = Vec::new();
    let mut seq = 1u64;
    while Instant::now() < deadline {
        let at = Instant::now();
        let row = (seq % 500) as u32;
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                serde_json::json!({
                    "type": "submit",
                    "client": me,
                    "seq": seq,
                    "base": { "revision": revision },
                    "ops": [{
                        "op": { "setValue": { "sheet": 0, "at": { "row": row, "col": client },
                                              "value": { "number": seq as f64 } } },
                        "formulas": {}, "styles": {}, "strings": {}
                    }]
                })
                .to_string(),
            ))
            .await?;
        sent += 1;
        seq += 1;

        // The reply that matters is the one carrying a new revision. Presence
        // and drafts arrive unsolicited and are not answers to this.
        while let Some(Ok(message)) = socket.next().await {
            let tokio_tungstenite::tungstenite::Message::Text(text) = message else {
                continue;
            };
            let value: serde_json::Value = serde_json::from_str(&text)?;
            if matches!(
                value["type"].as_str(),
                Some("presence" | "departed" | "draft")
            ) {
                continue;
            }
            if let Some(next) = value["revision"].as_u64() {
                revision = next;
            }
            latencies.push(at.elapsed().as_micros() as u64);
            break;
        }
        // A person does not type continuously. Two edits a second is a brisk
        // but real rate, and saturating the socket would measure the socket.
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Ok((sent, latencies))
}

/// A token, the way an integrator's host would mint one.
fn mint(
    secret: &str,
    audience: &str,
    origin: &str,
    key: &str,
    client: usize,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let claims = serde_json::json!({
        "iss": "opencalc-loadtest",
        "aud": audience,
        "iat": now,
        "exp": now + 3600,
        "user": { "id": format!("u-{client}"), "name": format!("Load {client}") },
        "document": {
            "key": key,
            "id": key,
            "title": "load.xlsx",
            "url": format!("{origin}/load.xlsx"),
        },
        "permissions": { "access": "edit" },
    });
    Ok(jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The gauge is read from a real exposition body.**
    ///
    /// The harness is useless if it cannot find the number, and it can only be
    /// tested against a running server on a machine that has one. A parser
    /// tested against a literal works anywhere.
    #[test]
    fn the_resident_gauge_is_found_among_the_others() {
        let body = "\
# HELP opencalc_documents Documents held on this node.
# TYPE opencalc_documents gauge
opencalc_documents 12
# HELP opencalc_resident_bytes Resident set size of this node.
# TYPE opencalc_resident_bytes gauge
opencalc_resident_bytes 52428800
# TYPE opencalc_participants gauge
opencalc_participants 36
";
        assert_eq!(gauge(body, "opencalc_resident_bytes"), Some(52_428_800));
        assert_eq!(gauge(body, "opencalc_documents"), Some(12));
        assert_eq!(gauge(body, "opencalc_absent"), None);
    }

    /// **A `# HELP` line is not a value.**
    ///
    /// The help text repeats the metric name, so a parser that took the first
    /// line starting with it would read the description and find no number —
    /// or worse, a number inside the prose.
    #[test]
    fn the_help_line_is_not_mistaken_for_the_value() {
        let body = "# HELP opencalc_resident_bytes Resident set size, 999 of them.\nopencalc_resident_bytes 4096\n";
        assert_eq!(gauge(body, "opencalc_resident_bytes"), Some(4096));
    }

    /// **p99 is a measurement somebody experienced**, not an interpolation
    /// between two that nobody did.
    #[test]
    fn the_percentile_is_a_real_sample() {
        let sorted: Vec<u64> = (1..=100).collect();
        assert_eq!(percentile(&sorted, 50.0), 50);
        assert_eq!(percentile(&sorted, 99.0), 99);
        assert_eq!(percentile(&sorted, 100.0), 100);
        assert_eq!(percentile(&[], 99.0), 0);
        assert_eq!(percentile(&[7], 99.0), 7);
    }
}

/// Whether a completed run is a result or the absence of one.
///
/// Run against no server at all, this harness printed a report of zeroes and
/// exited **0**. Wired into CI that way it would be a job that goes green
/// whether the collaboration server works, is broken, or was never started —
/// the same shape as a submission the server silently drops, which this project
/// has now paid for more than once.
///
/// Separate from `main` because that is what makes it testable: the failure
/// being guarded against is a *successful-looking* run, and a test that has to
/// stand up a server to check it would never be written.
fn verdict(sent: u64, acknowledged: u64) -> Result<(), String> {
    if acknowledged == 0 {
        return Err(
            "no edit was acknowledged: the harness measured nothing, so its \
                    zeroes are the absence of a result rather than a result"
                .to_owned(),
        );
    }
    // The percentiles are computed only over edits that came back, so a server
    // dropping work under load is *invisible* in them — the surviving edits
    // just look fast. This is the one place that difference can be seen.
    if acknowledged < sent {
        return Err(format!(
            "{} of {sent} edits were never acknowledged: the server dropped work under load",
            sent - acknowledged
        ));
    }
    Ok(())
}

#[cfg(test)]
mod verdict_tests {
    use super::verdict;

    /// **A run that measured nothing is a failure, not a pass.**
    ///
    /// This is the whole point: before it, `cargo run -p casual-calc-loadtest`
    /// against nothing at all exited 0.
    #[test]
    fn measuring_nothing_is_not_success() {
        let why = verdict(0, 0).expect_err("a run that acknowledged nothing reported success");
        assert!(why.contains("measured nothing"), "{why}");
        // And it stays a failure even when edits were sent — that is the
        // "server refused every join" case, which is what actually happened.
        assert!(
            verdict(6_000, 0).is_err(),
            "every edit lost, yet reported success"
        );
    }

    /// **Dropped work fails, even though the percentiles would look healthy.**
    #[test]
    fn dropped_edits_are_a_failure() {
        let why = verdict(6_000, 5_999).expect_err("a dropped edit reported success");
        assert!(why.contains("1 of 6000"), "{why}");
        assert!(why.contains("dropped work"), "{why}");
    }

    /// **A clean run passes**, or the gate is just noise.
    #[test]
    fn a_complete_run_passes() {
        assert!(verdict(6_000, 6_000).is_ok());
    }
}
