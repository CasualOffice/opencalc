//! MCP over stdio: JSON-RPC 2.0, one message per line.
//!
//! # The protocol as implemented
//!
//! Written against the **initialization-based ("legacy") MCP revisions**,
//! newest first `2025-11-25`, then `2025-06-18` and `2025-03-26`. These are
//! the revisions in which every shipping MCP client (Claude Desktop, Claude
//! Code) opens a stdio session with `initialize`. The `2026-07-28` revision
//! replaced the handshake with per-request `_meta` and `server/discover`; its
//! own compatibility rules have a dual-era client probe `server/discover`
//! first and fall back to `initialize` on any non-modern error, which the
//! `-32601 Method not found` this server returns is. Serving the stateless
//! revision as well is tracked separately rather than half-done here.
//!
//! **Transport (stdio).** The client writes requests to our stdin and reads
//! our stdout. Each message is one line of UTF-8 JSON, no embedded newline.
//! Nothing but MCP messages is ever written to stdout; diagnostics go to
//! stderr. A line longer than [`MAX_LINE_BYTES`] is answered with an
//! `Invalid Request` error and skipped, so a runaway client cannot make the
//! server buffer without bound.
//!
//! **Messages handled.**
//!
//! | Message | Shape |
//! |---|---|
//! | `initialize` (request) | params `{protocolVersion, capabilities, clientInfo}` → result `{protocolVersion, capabilities: {tools: {listChanged: false}}, serverInfo: {name, title, version}, instructions}`. The version is the client's when we support it, otherwise our newest; the client decides whether to continue. |
//! | `notifications/initialized` | notification; no reply. |
//! | `ping` (request) | result `{}`. |
//! | `tools/list` (request) | result `{tools: [{name, title, description, inputSchema, annotations}]}`; one page, no `nextCursor`. |
//! | `tools/call` (request) | params `{name, arguments}` → result `{content: [{type: "text", text}], isError}`. `text` is compact JSON. A failure *of the tool* (bad arguments, file refused, formula error) is a result with `isError: true`, so the model sees it and can correct itself; an unknown tool name is a JSON-RPC `-32602`. |
//! | `notifications/cancelled` and any other notification | ignored; no reply. |
//!
//! **Errors.** Unparseable JSON → `-32700` with `id: null`. A batch (JSON
//! array, removed from MCP in `2025-06-18`), a non-object, a missing
//! `jsonrpc: "2.0"` or a missing method → `-32600`. An unknown method →
//! `-32601`. Bad params for a protocol method → `-32602`. A message with no
//! `method` but a `result` or `error` is a response to a request the client
//! thinks we sent; this server sends none, so it is ignored.
//!
//! **No panic reaches the transport.** A tool is run under `catch_unwind`;
//! a panic becomes an `isError` result naming an internal error, and the
//! session continues.

use std::io::{self, BufRead, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};

use serde_json::{Value, json};

use crate::tools::{Call, Tools};

/// Protocol revisions this server speaks, newest first.
pub const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];

/// The longest message line accepted. Generous for any real tool call — a
/// 10,000-cell `write_cells` is well under a megabyte — and a bound on what a
/// client can make the server hold.
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

const INSTRUCTIONS: &str = "OpenCalc spreadsheet tools. Open a workbook with open_workbook \
(paths must be inside the server's allowed roots), then use the returned handle with the other \
tools. Addresses are A1 (B7, A1:C10, Sheet2!A1:B3). write_cells reads strings exactly as a \
person typing into Excel would ('=SUM(A1:A3)' is a formula, '50%' a percentage, a leading \
apostrophe forces text). Nothing is written to disk until save; save returns what the chosen \
format cannot keep.";

/// One MCP server session.
#[derive(Debug)]
pub struct Server {
    tools: Tools,
    protocol_version: Option<String>,
}

impl Server {
    /// A session over `tools`.
    pub fn new(tools: Tools) -> Self {
        Self {
            tools,
            protocol_version: None,
        }
    }

    /// The protocol revision `initialize` settled on, if it has run.
    pub fn protocol_version(&self) -> Option<&str> {
        self.protocol_version.as_deref()
    }

    /// Handle one line, returning the reply line, or `None` when the message
    /// was a notification (or otherwise wants no reply).
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let message: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return Some(error(
                    &Value::Null,
                    PARSE_ERROR,
                    &format!("Parse error: {e}"),
                ));
            }
        };
        let reply = self.handle(&message)?;
        Some(reply)
    }

    fn handle(&mut self, message: &Value) -> Option<String> {
        let Some(obj) = message.as_object() else {
            let why = if message.is_array() {
                "Invalid Request: batches are not supported"
            } else {
                "Invalid Request: a message must be a JSON object"
            };
            return Some(error(&Value::Null, INVALID_REQUEST, why));
        };
        // An id must be a string or a number; anything else cannot be echoed
        // back meaningfully, so the reply goes to `null`.
        let id = obj.get("id").cloned();
        let reply_id = match &id {
            Some(v @ (Value::String(_) | Value::Number(_))) => v.clone(),
            _ => Value::Null,
        };
        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            if obj.contains_key("result") || obj.contains_key("error") {
                return None;
            }
            return Some(error(
                &reply_id,
                INVALID_REQUEST,
                "Invalid Request: no method",
            ));
        };
        // A notification is never answered, not even with an error — the
        // client is not waiting for one. None this server receives
        // (`notifications/initialized`, `notifications/cancelled`) needs
        // acting on: every tool call completes before the next line is read.
        let id = id?;
        if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Some(error(
                &reply_id,
                INVALID_REQUEST,
                "Invalid Request: jsonrpc must be \"2.0\"",
            ));
        }
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        if !matches!(id, Value::String(_) | Value::Number(_)) {
            return Some(error(
                &Value::Null,
                INVALID_REQUEST,
                "Invalid Request: id must be a string or a number",
            ));
        }
        let outcome = match method {
            "initialize" => Ok(self.initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": Tools::definitions() })),
            "tools/call" => self.call(&params),
            _ => Err((METHOD_NOT_FOUND, format!("Method not found: {method}"))),
        };
        Some(match outcome {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string(),
            Err((code, msg)) => error(&id, code, &msg),
        })
    }

    fn initialize(&mut self, params: &Value) -> Value {
        let requested = params.get("protocolVersion").and_then(Value::as_str);
        let version = match requested {
            Some(v) if SUPPORTED_VERSIONS.contains(&v) => v,
            _ => SUPPORTED_VERSIONS[0],
        };
        self.protocol_version = Some(version.to_owned());
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": {
                "name": "casual-calc-mcp",
                "title": "OpenCalc",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": INSTRUCTIONS,
        })
    }

    fn call(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params.get("name").and_then(Value::as_str).ok_or((
            INVALID_PARAMS,
            "tools/call needs a string `name`".to_owned(),
        ))?;
        let empty = Value::Object(serde_json::Map::new());
        let args = match params.get("arguments") {
            None | Some(Value::Null) => &empty,
            Some(v @ Value::Object(_)) => v,
            Some(_) => {
                return Ok(tool_result(
                    &json!({ "error": "arguments must be a JSON object" }),
                    true,
                ));
            }
        };
        let tools = &mut self.tools;
        let ran = catch_unwind(AssertUnwindSafe(|| tools.call(name, args)));
        match ran {
            Ok(Call::Unknown) => Err((INVALID_PARAMS, format!("Unknown tool: {name}"))),
            Ok(Call::Done(Ok(value))) => Ok(tool_result(&value, false)),
            Ok(Call::Done(Err(message))) => Ok(tool_result(&json!({ "error": message }), true)),
            Err(_) => Ok(tool_result(
                &json!({ "error": format!("internal error while running {name}; the workbook handle may need reopening") }),
                true,
            )),
        }
    }
}

fn tool_result(value: &Value, is_error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": value.to_string() }],
        "isError": is_error,
    })
}

fn error(id: &Value, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
    .to_string()
}

/// Read one line of at most [`MAX_LINE_BYTES`] into `buf` (newline stripped).
///
/// `Ok(None)` at end of input; `Ok(Some(false))` for a line that was too long
/// and has been skipped through its newline.
fn read_line(input: &mut impl BufRead, buf: &mut Vec<u8>) -> io::Result<Option<bool>> {
    buf.clear();
    let limit = (MAX_LINE_BYTES + 1) as u64;
    let n = std::io::Read::take(&mut *input, limit).read_until(b'\n', buf)?;
    if n == 0 {
        return Ok(None);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
        return Ok(Some(true));
    }
    if buf.len() <= MAX_LINE_BYTES {
        // The last line, without a newline.
        return Ok(Some(true));
    }
    // Too long: drain the rest of the line in bounded pieces.
    loop {
        buf.clear();
        let n = std::io::Read::take(&mut *input, limit).read_until(b'\n', buf)?;
        if n == 0 || buf.last() == Some(&b'\n') {
            break;
        }
    }
    buf.clear();
    Ok(Some(false))
}

/// Serve MCP over `input`/`output` until `input` ends.
///
/// # Errors
///
/// Only an I/O error on the streams themselves; every protocol or tool
/// failure is answered on the wire.
pub fn serve(
    server: &mut Server,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> io::Result<()> {
    let mut buf = Vec::new();
    while let Some(fits) = read_line(input, &mut buf)? {
        let reply = if !fits {
            Some(error(
                &Value::Null,
                INVALID_REQUEST,
                &format!("Invalid Request: message longer than {MAX_LINE_BYTES} bytes"),
            ))
        } else {
            match std::str::from_utf8(&buf) {
                Ok(line) if line.trim().is_empty() => None,
                Ok(line) => server.handle_line(line),
                Err(_) => Some(error(&Value::Null, PARSE_ERROR, "Parse error: not UTF-8")),
            }
        };
        if let Some(reply) = reply {
            output.write_all(reply.as_bytes())?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::Sandbox;

    fn server() -> Server {
        Server::new(Tools::new(Sandbox::default()))
    }

    fn reply(s: &mut Server, line: &str) -> Value {
        serde_json::from_str(&s.handle_line(line).expect("a reply")).unwrap()
    }

    #[test]
    fn malformed_json_is_a_parse_error_with_a_null_id() {
        let r = reply(&mut server(), "{\"jsonrpc\":\"2.0\",");
        assert_eq!(r["error"]["code"], -32700);
        assert_eq!(r["id"], Value::Null);
    }

    #[test]
    fn a_batch_and_a_bad_envelope_are_invalid_requests() {
        let mut s = server();
        assert_eq!(reply(&mut s, "[]")["error"]["code"], -32600);
        assert_eq!(reply(&mut s, "7")["error"]["code"], -32600);
        let r = reply(&mut s, r#"{"jsonrpc":"1.0","id":3,"method":"ping"}"#);
        assert_eq!(r["error"]["code"], -32600);
        assert_eq!(r["id"], 3);
    }

    #[test]
    fn initialize_negotiates_the_version() {
        let mut s = server();
        let r = reply(
            &mut s,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        );
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(r["result"]["capabilities"]["tools"]["listChanged"], false);
        assert_eq!(r["result"]["serverInfo"]["name"], "casual-calc-mcp");
        let r = reply(
            &mut s,
            r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
        );
        assert_eq!(r["result"]["protocolVersion"], SUPPORTED_VERSIONS[0]);
    }

    #[test]
    fn notifications_and_responses_get_no_reply() {
        let mut s = server();
        assert!(
            s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .is_none()
        );
        assert!(
            s.handle_line(r#"{"jsonrpc":"2.0","id":9,"result":{}}"#)
                .is_none()
        );
    }

    #[test]
    fn unknown_methods_and_tools_are_protocol_errors() {
        let mut s = server();
        let r = reply(
            &mut s,
            r#"{"jsonrpc":"2.0","id":1,"method":"server/discover"}"#,
        );
        assert_eq!(r["error"]["code"], -32601);
        let r = reply(
            &mut s,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"rm_rf","arguments":{}}}"#,
        );
        assert_eq!(r["error"]["code"], -32602);
    }

    #[test]
    fn a_tool_failure_is_an_is_error_result() {
        let mut s = server();
        let r = reply(
            &mut s,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"open_workbook","arguments":{"path":"/etc/passwd"}}}"#,
        );
        assert_eq!(r["result"]["isError"], true);
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("--root"), "{text}");
    }

    #[test]
    fn the_stream_skips_an_oversized_line_and_carries_on() {
        let mut s = server();
        let mut input = Vec::new();
        input.extend(std::iter::repeat_n(b'x', MAX_LINE_BYTES + 10));
        input.push(b'\n');
        input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n");
        input.extend_from_slice(b"\xff\xfe\n");
        let mut out = Vec::new();
        serve(&mut s, &mut &input[..], &mut out).unwrap();
        let lines: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["error"]["code"], -32600);
        assert_eq!(lines[1]["result"], json!({}));
        assert_eq!(lines[2]["error"]["code"], -32700);
    }
}
