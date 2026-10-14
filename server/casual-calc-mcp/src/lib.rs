//! `casual-calc-mcp` — an MCP server that lets an AI agent drive a workbook
//! (docs/97, `AGENT-01`).
//!
//! One process, one client, stdio. The client launches the binary, speaks
//! JSON-RPC 2.0 on its stdin/stdout ([`protocol`]) and calls the tools in
//! [`tools`], each a thin wrapper over a `casual-calc-sdk` call.
//!
//! # Security bounds (docs/97 — part of the contract)
//!
//! - **Filesystem roots.** Files are read and written only under the
//!   directories given with `--root` ([`sandbox`]). Paths are canonicalised —
//!   `..` and symlinks resolved by the OS — before the check. No root, no file
//!   access at all.
//! - **No network.** There is no networking code in this crate or in what it
//!   depends on for its own work: no HTTP client, no sockets, no async runtime.
//! - **Bounds.** The SDK's file-size limit applies to every open (and can be
//!   lowered with `--max-file-bytes`, never raised); [`tools::MAX_CELLS`]
//!   cells per read or write; [`tools::MAX_HANDLES`] open workbooks; a message
//!   line of at most [`protocol::MAX_LINE_BYTES`].
//! - **No panic reaches the client.** A tool failure is an `isError` result;
//!   malformed JSON is a JSON-RPC parse error.

pub mod a1;
pub mod protocol;
pub mod sandbox;
pub mod tools;
