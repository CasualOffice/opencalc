//! The `casual-calc-mcp` binary: MCP over stdio. See the library docs.

use std::path::PathBuf;
use std::process::ExitCode;

use casual_calc_mcp::protocol::{Server, serve};
use casual_calc_mcp::sandbox::Sandbox;
use casual_calc_mcp::tools::Tools;

const USAGE: &str = "\
casual-calc-mcp — an MCP server (stdio) for OpenCalc workbooks

USAGE:
    casual-calc-mcp --root <DIR> [--root <DIR>]... [--max-file-bytes <N>]

OPTIONS:
    --root <DIR>            A directory the tools may read and write under.
                            Repeatable. Without one, every file tool refuses.
    --max-file-bytes <N>    Refuse to open files larger than N bytes (can only
                            lower the engine's own limit).
    -h, --help              Print this help.
    -V, --version           Print the version.

The server reads JSON-RPC 2.0 messages, one per line, on stdin and writes
replies on stdout. Diagnostics go to stderr.";

fn main() -> ExitCode {
    let mut roots = Vec::new();
    let mut max_file_bytes = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            Some("-V" | "--version") => {
                println!("casual-calc-mcp {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            Some("--root") => match args.next() {
                Some(dir) => roots.push(PathBuf::from(dir)),
                None => return usage_error("--root needs a directory"),
            },
            Some("--max-file-bytes") => {
                match args
                    .next()
                    .and_then(|n| n.to_str().and_then(|n| n.parse::<u64>().ok()))
                {
                    Some(n) => max_file_bytes = Some(n),
                    None => return usage_error("--max-file-bytes needs a number"),
                }
            }
            _ => {
                return usage_error(&format!("unexpected argument {}", arg.to_string_lossy()));
            }
        }
    }
    let sandbox = match Sandbox::new(&roots) {
        Ok(s) => s,
        Err(e) => return usage_error(&e),
    };
    if sandbox.roots().is_empty() {
        eprintln!("casual-calc-mcp: no --root given; every file tool will refuse");
    }
    let mut tools = Tools::new(sandbox);
    if let Some(n) = max_file_bytes {
        tools = tools.with_max_file_bytes(n);
    }
    let mut server = Server::new(tools);
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    match serve(&mut server, &mut stdin.lock(), &mut stdout.lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("casual-calc-mcp: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("casual-calc-mcp: {message}\n\n{USAGE}");
    ExitCode::from(2)
}
