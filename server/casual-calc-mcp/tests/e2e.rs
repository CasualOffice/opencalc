//! End-to-end: spawn the built binary and drive a real MCP session over its
//! stdin/stdout (docs/97, `AGENT-01` gate).
//!
//! open → read → write a formula → evaluate → save → reopen and confirm; a
//! path outside the roots refused; the compatibility and loss reports
//! returned.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use casual_calc_sdk::{CellRef, WorkbookSession};
use serde_json::{Value, json};

/// A scratch directory under cargo's per-target temp dir.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "mcp-e2e-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A live server process and a line-oriented client for it.
struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
}

impl Session {
    fn spawn(args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_casual-calc-mcp"))
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn casual-calc-mcp");
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let stdin = child.stdin.take();
        Self {
            child,
            stdin,
            lines: rx,
            next_id: 1,
        }
    }

    fn send_raw(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(line.as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    }

    fn recv(&mut self) -> Value {
        let line = self
            .lines
            .recv_timeout(Duration::from_secs(60))
            .expect("the server replied within 60s");
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("reply is not JSON ({e}): {line}"))
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.send_raw(&msg.to_string());
        let reply = self.recv();
        assert_eq!(reply["jsonrpc"], "2.0", "{reply}");
        assert_eq!(reply["id"], id, "reply to the wrong request: {reply}");
        reply
    }

    /// Call a tool; returns `(isError, parsed text payload)`.
    fn tool(&mut self, name: &str, arguments: Value) -> (bool, Value) {
        let reply = self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        let result = &reply["result"];
        assert!(
            result.is_object(),
            "tools/call {name} gave no result: {reply}"
        );
        let content = result["content"].as_array().expect("content array");
        assert_eq!(content[0]["type"], "text");
        let text = content[0]["text"].as_str().unwrap();
        let payload: Value = serde_json::from_str(text).expect("text is compact JSON");
        (result["isError"] == true, payload)
    }

    fn ok(&mut self, name: &str, arguments: Value) -> Value {
        let (is_error, payload) = self.tool(name, arguments.clone());
        assert!(!is_error, "{name}({arguments}) failed: {payload}");
        payload
    }

    fn initialize(&mut self) -> Value {
        let reply = self.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "e2e", "version": "0" }
            }),
        );
        self.send_raw(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        reply
    }

    fn finish(mut self) {
        drop(self.stdin.take());
        let status = self.child.wait().unwrap();
        assert!(status.success(), "server exited with {status}");
    }
}

/// A small workbook: A1..A3 = 1, 2, 3 and B1 = "Name".
fn fixture(path: &Path) {
    let mut s = WorkbookSession::with_sheet();
    for (row, input) in [(0, "1"), (1, "2"), (2, "3")] {
        let op = s.input_edit(0, CellRef::new(row, 0), input);
        s.edit(op).unwrap();
    }
    let op = s.input_edit(0, CellRef::new(0, 1), "Name");
    s.edit(op).unwrap();
    std::fs::write(path, s.save().unwrap()).unwrap();
}

#[test]
fn an_agent_opens_reads_writes_evaluates_and_saves_a_workbook() {
    let root = Scratch::new("root");
    let elsewhere = Scratch::new("elsewhere");
    let book = root.0.join("book.xlsx");
    fixture(&book);
    std::fs::create_dir_all(root.0.join("out")).unwrap();

    let mut s = Session::spawn(&["--root", root.0.to_str().unwrap()]);

    // initialize
    let init = s.initialize();
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18", "{init}");
    assert_eq!(init["result"]["serverInfo"]["name"], "casual-calc-mcp");
    assert!(init["result"]["capabilities"]["tools"].is_object());

    // tools/list — the docs/97 table, each with a schema and a description
    let list = s.request("tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().expect("tools");
    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "close",
            "evaluate",
            "list_sheets",
            "open_workbook",
            "read_range",
            "recalculate",
            "run",
            "save",
            "write_cells"
        ]
    );
    for t in tools {
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        assert!(t["inputSchema"]["properties"].is_object(), "{t}");
        assert!(t["description"].as_str().unwrap().len() > 40, "{t}");
    }

    // open — returns a handle, the sheets and the import compatibility report
    let opened = s.ok("open_workbook", json!({ "path": book.to_str().unwrap() }));
    let wb = opened["handle"].as_str().unwrap().to_owned();
    assert_eq!(opened["format"], "xlsx");
    assert_eq!(opened["sheets"][0]["used_range"], "A1:B3");
    assert!(
        opened["compatibility_report"]["entries"].is_array(),
        "no compatibility report: {opened}"
    );

    // read
    let read = s.ok("read_range", json!({ "handle": wb, "range": "A1:B3" }));
    assert_eq!(
        read["rows"],
        json!([[1.0, "Name"], [2.0, null], [3.0, null]])
    );
    assert_eq!(read["truncated"], false);

    // write a formula; the result comes back computed
    let wrote = s.ok(
        "write_cells",
        json!({ "handle": wb, "anchor": "A4", "values": [["=SUM(A1:A3)", "50%"]] }),
    );
    assert_eq!(wrote["written"], 2, "{wrote}");
    assert_eq!(wrote["refused"], json!([]));
    assert_eq!(wrote["values"], json!([[6.0, 0.5]]));

    // a formula that does not parse is refused, with the reason, and the
    // rest of the call is still written
    let partial = s.ok(
        "write_cells",
        json!({ "handle": wb, "anchor": "C1", "values": [["=SUM(1,", "kept"]] }),
    );
    assert_eq!(partial["written"], 1, "{partial}");
    assert_eq!(partial["refused"][0]["cell"], "C1");
    assert!(
        partial["refused"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("apostrophe")
    );

    // evaluate without writing
    let ev = s.ok("evaluate", json!({ "handle": wb, "formula": "=A1*2" }));
    assert_eq!(ev["value"], 2.0);
    let ev = s.ok("evaluate", json!({ "handle": wb, "formula": "A4*10" }));
    assert_eq!(ev["value"], 60.0);
    let ev = s.ok("evaluate", json!({ "handle": wb, "formula": "=1/0" }));
    assert_eq!(ev["value"], json!({ "error": "#DIV/0!" }));
    let ev = s.ok("evaluate", json!({ "handle": wb, "formula": "=A1:A2*3" }));
    assert_eq!(ev["value"], json!({ "array": [[3.0], [6.0]] }));
    let (bad, why) = s.tool("evaluate", json!({ "handle": wb, "formula": "=SUM(" }));
    assert!(
        bad,
        "an unparseable formula must be an isError result: {why}"
    );

    // read back the computed value, the formula and the display text
    let both = s.ok(
        "read_range",
        json!({ "handle": wb, "range": "Sheet1!A4:B4", "mode": "both" }),
    );
    assert_eq!(both["rows"][0][0]["value"], 6.0);
    assert_eq!(both["rows"][0][0]["formula"], "=SUM(A1:A3)");
    assert_eq!(both["rows"][0][1]["display"], "50%");

    // save to a new path inside the root — the loss report comes back
    let saved_path = root.0.join("out/saved.xlsx");
    let saved = s.ok(
        "save",
        json!({ "handle": wb, "path": saved_path.to_str().unwrap() }),
    );
    assert_eq!(saved["format"], "xlsx");
    assert!(
        saved["loss_report"]["entries"].is_array(),
        "no loss report: {saved}"
    );
    assert_eq!(saved["loss_report"]["lossless"], true, "{saved}");
    assert!(saved_path.is_file());

    // re-open the saved file: the formula and its value survived
    let again = s.ok("open_workbook", json!({ "path": "out/saved.xlsx" }));
    let wb2 = again["handle"].as_str().unwrap().to_owned();
    assert_ne!(wb, wb2);
    let f = s.ok(
        "read_range",
        json!({ "handle": wb2, "range": "A4", "mode": "formulas" }),
    );
    assert_eq!(f["rows"], json!([["=SUM(A1:A3)"]]));
    let v = s.ok("read_range", json!({ "handle": wb2, "range": "A4" }));
    assert_eq!(v["rows"], json!([[6.0]]));

    // a save to a lossy format says what it costs
    let csv = s.ok(
        "save",
        json!({ "handle": wb2, "path": root.0.join("out/flat.csv").to_str().unwrap() }),
    );
    assert_eq!(csv["format"], "csv");
    assert_eq!(csv["loss_report"]["lossless"], false, "{csv}");
    assert!(!csv["loss_report"]["entries"].as_array().unwrap().is_empty());

    // a format that contradicts the file name is refused
    let (bad, why) = s.tool(
        "save",
        json!({ "handle": wb, "path": root.0.join("out/x.xlsx").to_str().unwrap(), "format": "csv" }),
    );
    assert!(bad, "{why}");

    // outside the roots: refused for save and for open, nothing written
    let outside = elsewhere.0.join("stolen.xlsx");
    let (bad, why) = s.tool(
        "save",
        json!({ "handle": wb, "path": outside.to_str().unwrap() }),
    );
    assert!(bad, "an out-of-root save must be an isError result: {why}");
    assert!(why["error"].as_str().unwrap().contains("outside"), "{why}");
    assert!(!outside.exists(), "the out-of-root save wrote a file");
    let escape = root
        .0
        .join("../")
        .join(elsewhere.0.file_name().unwrap())
        .join("x.xlsx");
    let (bad, _) = s.tool(
        "save",
        json!({ "handle": wb, "path": escape.to_str().unwrap() }),
    );
    assert!(bad, "a .. escape must be refused");
    std::fs::write(
        elsewhere.0.join("secret.xlsx"),
        std::fs::read(&book).unwrap(),
    )
    .unwrap();
    let (bad, why) = s.tool(
        "open_workbook",
        json!({ "path": elsewhere.0.join("secret.xlsx").to_str().unwrap() }),
    );
    assert!(bad, "an out-of-root open must be refused: {why}");

    // structural edit: insert a row above, and the formula follows
    let ran = s.ok(
        "run",
        json!({ "handle": wb, "op": "insert_rows", "at": 1, "count": 1 }),
    );
    assert_eq!(ran["lines"], "1:1");
    let f = s.ok(
        "read_range",
        json!({ "handle": wb, "range": "A5", "mode": "formulas" }),
    );
    assert_eq!(f["rows"], json!([["=SUM(A2:A4)"]]));
    let (bad, why) = s.tool("run", json!({ "handle": wb, "op": "sort" }));
    assert!(
        bad && why["error"].as_str().unwrap().contains("sort"),
        "{why}"
    );

    // malformed JSON is a JSON-RPC parse error, and the session carries on
    s.send_raw("{\"jsonrpc\": \"2.0\", \"id\": 99, \"method\": ");
    let parse = s.recv();
    assert_eq!(parse["error"]["code"], -32700, "{parse}");
    assert_eq!(parse["id"], Value::Null);
    let ping = s.request("ping", json!({}));
    assert_eq!(ping["result"], json!({}));

    // a formula that panics inside the engine (SEQUENCE's rows×cols
    // overflows i64) does not take the server down: the call fails or
    // answers an error value, and the next request is still served
    let (_, payload) = s.tool(
        "evaluate",
        json!({ "handle": wb, "formula": "=SEQUENCE(1E10,1E10)" }),
    );
    assert!(
        payload.get("error").is_some() || payload["value"].get("error").is_some(),
        "{payload}"
    );
    let still = s.ok("evaluate", json!({ "handle": wb, "formula": "=1+1" }));
    assert_eq!(still["value"], 2.0);

    // close; a closed handle is gone
    s.ok("close", json!({ "handle": wb }));
    let (bad, _) = s.tool("list_sheets", json!({ "handle": wb }));
    assert!(bad);
    s.ok("close", json!({ "handle": wb2 }));

    s.finish();
}

#[test]
fn without_a_root_every_file_tool_refuses() {
    let dir = Scratch::new("noroot");
    let book = dir.0.join("book.xlsx");
    fixture(&book);
    let mut s = Session::spawn(&[]);
    s.initialize();
    let (bad, why) = s.tool("open_workbook", json!({ "path": book.to_str().unwrap() }));
    assert!(bad, "{why}");
    assert!(why["error"].as_str().unwrap().contains("--root"), "{why}");
    s.finish();
}

#[test]
fn open_handles_are_capped_and_oversized_files_refused() {
    let root = Scratch::new("cap");
    let book = root.0.join("book.xlsx");
    fixture(&book);
    let size = std::fs::metadata(&book).unwrap().len();
    let mut s = Session::spawn(&["--root", root.0.to_str().unwrap()]);
    s.initialize();
    for _ in 0..16 {
        s.ok("open_workbook", json!({ "path": "book.xlsx" }));
    }
    let (bad, why) = s.tool("open_workbook", json!({ "path": "book.xlsx" }));
    assert!(bad, "a 17th handle must be refused: {why}");
    s.finish();

    let limit = (size - 1).to_string();
    let mut s = Session::spawn(&[
        "--root",
        root.0.to_str().unwrap(),
        "--max-file-bytes",
        &limit,
    ]);
    s.initialize();
    let (bad, why) = s.tool("open_workbook", json!({ "path": "book.xlsx" }));
    assert!(bad, "a file over the size limit must be refused: {why}");
    assert!(why["error"].as_str().unwrap().contains("limit"), "{why}");
    s.finish();
}
