//! The OpenCalc collaboration server — **a session, not a filing system**.
//!
//! Implements [ADR-012](../../../docs/57-COLLABORATION-SERVER-BOUNDARY.md) on
//! top of the protocol in
//! [ADR-011](../../../docs/56-COLLABORATION-CONCURRENCY-DESIGN.md). The
//! integrator keeps the document of record; this coordinates the editing of it
//! and hands finished bytes back through a webhook.
//!
//! # Why this is a separate crate
//!
//! It is a **host**, not a layer. AGENTS.md's rule is that the engine computes
//! and the host decides I/O, network and persistence, so a network service does
//! not belong inside a dependency graph that currently contains no async
//! runtime and no HTTP stack. It may depend on `crates/`; **nothing under
//! `crates/` may depend on it**, and CI enforces that.
//!
//! # Why the policy here has no I/O either
//!
//! Everything in this crate is a state machine over supplied time and supplied
//! bytes. Nothing reads a clock, opens a socket or awaits. That is the same
//! discipline the engine follows, for the same payoff: a save cadence and a
//! retry policy are exactly the things whose bugs live in rare timing, and they
//! are only testable if time is an argument.

#![forbid(unsafe_code)]

pub mod cluster;
pub mod config;
pub mod document;
pub mod http;
pub mod lifecycle;
pub mod net;
pub mod presence;
pub mod token;
pub mod verify;

pub use cluster::redis::Redis;
pub use cluster::{AppendError, Coordinator, Lease, Memory, Peer, Unavailable, elect};
pub use config::{Endpoint, Exposure, ProxyTrust, TlsFiles};
pub use document::{DocumentSession, Joined, ServerError};
pub use lifecycle::{Action, CallbackOutcome, SavePolicy, SaveReason, SessionLifecycle};
pub use presence::{Presence, Roster};
pub use token::{Access, Callback, Claims, Document, Permissions, TokenPolicy, User};
pub use verify::{KeySet, Signing, Verifier, VerifyError};
