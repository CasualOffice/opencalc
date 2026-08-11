# 60 — Production readiness: what is missing, and what breaks

An audit of the whole application against the standard the project claims for
itself — "correctness, determinism, security bounds and fidelity before
performance". It exists because the engine is in far better shape than the
things around it, and a strong engine inside a service that loses data is a
demo.

**How to read the evidence column.** `Verified` means it was made to happen.
`Observed` means it was read out of the code and not run. That distinction has
earned its place: in this repository, claims made from reading have repeatedly
turned out wrong, and claims from running have held.

---

## S1 — Data loss and unrecoverable crashes

### S1-1 · A nested formula aborts the process · **Verified**

```text
depth   100: parsed ok
depth 1,000: parsed ok
depth 20,000: fatal runtime error: stack overflow, aborting
```

`casual-calc-formula`'s parser is recursive descent with **no depth limit**. The
evaluator has one (`MAX_DEPTH = 256`) and the XML reader has one
(`max_xml_depth = 256`); the parser between them has none.

A stack overflow in Rust is `SIGABRT`. It is not an `Err`, not a catchable
panic, and not something a `Result` signature can express. Where it lands:

| Caller | Consequence |
| --- | --- |
| `import` parsing `<f>` from a file (`lib.rs:619`) | Opening a hostile `.xlsx` kills the process |
| The collaboration server | **One document kills the node** — every other document, every other user |
| The WASM editor | The tab dies |
| `validate_formula` on typed input | Same, from the formula bar |

The package fuzz target covers ZIP admission and stops there, so nothing was
ever going to find this. **The parser needs a depth bound, and the fuzz corpus
needs a formula target.**

### S1-2 · The collaboration server never saves · **Observed**

`DocumentSession::tick`, `::callback` and `::assemble` are **never called** from
`net.rs`. The save lifecycle — quiesce timer, ceiling, revision cadence,
callback retry with backoff, read-only fencing — is fully built and fully tested
and nothing drives it.

So the service today accepts edits, orders them, transforms them and broadcasts
them, and then holds the only copy in memory. A restart loses every unsaved
edit; the integrator's callback is never called; the document of record never
changes. Everything *looks* right, which is the worst version of this.

The gap is a scheduler: a per-document timer that calls `tick`, acts on the
returned `Action`, and an HTTP client for the callback (S3-4).

---

## S2 — Availability and correctness under load

### S2-1 · A panic while holding a lock breaks a document forever · **Observed**

Eleven `Mutex::lock().expect(…)` in `net.rs`. Rust poisons a mutex when a thread
panics holding it, and every later `lock()` then returns `Err` — which these
turn into another panic.

The blast radius depends on which lock: `session` or `roster` breaks **that
document** permanently; `registry` breaks **every document on the node**. The
process keeps serving, so nothing restarts it and no health check notices.

A server should treat a poisoned lock as a recoverable event — take the inner
value, log loudly, and continue — or use a mutex that does not poison.

### S2-2 · Documents are never evicted · **Observed**

Nothing removes an entry from `Registry::live`. A node that has served a
thousand documents holds a thousand workbooks, with their snapshots and history,
for the life of the process. There is no idle timeout, no cap, and no memory
ceiling. It ends in the OOM killer, which looks like a crash and reads like a
mystery.

### S2-3 · Presence never expires · **Observed**

`Roster::expire` exists, is tested, and is never called. Its own doc comment
explains why that matters: *"a cursor that stops moving and never disappears is
worse than one that never appeared, since it reads as somebody watching."*

### S2-4 · No limits on anything a client controls · **Observed**

No cap on: connections per node, connections per document, connections per user,
submission rate, operations per submission, or WebSocket frame size. One client
can open connections until the node runs out of descriptors, or submit until it
saturates a document's lock.

### S2-5 · No graceful shutdown · **Observed**

No `SIGTERM` handling. A rolling deploy drops every connection mid-edit and
loses everything unsaved — which, per S1-2, is currently everything.

---

## S3 — Operability

### S3-1 · No observability · **Observed**

No tracing, no metrics, no structured logs. The service's total telemetry is one
`eprintln!` in the join path. ADR-012 lists telemetry among the reasons the
server exists at all.

Nothing reports: documents open, participants, revisions committed, transform
depth, save successes and failures, callback retries, or how long a save took.
An operator cannot answer "is it working" except by trying it.

### S3-2 · TLS is configured and not wired · **Observed**

`Exposure`, `Endpoint` and `TlsFiles` model plain, TLS and mutual TLS with
validation and startup warnings. `serve()` binds a plain `TcpListener` and
ignores all of it. **Right now the TLS choice is documentation.**

### S3-3 · Cluster mode is unbuilt · **Observed**

No Redis dependency and no code for it. Absent: node registration and refresh,
discovery, the leader lease, epoch fencing, the relay, the Streams log, and
election. ADR-012 and ADR-014 describe it; the service is standalone-only.

Standalone is a legitimate first-class mode — but "horizontally scalable" is not
true today, and that was a core requirement.

### S3-4 · The callback sender does not exist · **Observed**

The lifecycle decides *when* to save and with what retry policy. Nothing
performs the request. Both callback shapes — OnlyOffice URL and WOPI `PutFile` —
are modelled in the token and neither is implemented.

### S3-5 · Overflow checks are off in release · **Observed**

`[profile.release]` sets `lto`, `codegen-units` and `strip`, and does not set
`overflow-checks`. Arithmetic that panics in a debug test wraps silently in a
release build — so arithmetic that overflows is caught by a debug
test and ships wrong.

### S3-6 · Half the editor's error handling is silent · **Observed**

82 of 161 `catch` blocks in `webapp/editor.js` are empty. That pattern is what
hid UX-B04's undo bug behind `try { wasm.session_undo(); } catch {}` until the
browser gate found it.

---

## S4 — What the gates do not cover

### S4-1 · Fuzzing reaches one component · **Observed**

The single target covers ZIP/OPC admission. Not fuzzed: the formula parser
(S1-1), the XML readers, the number-format interpreter, the OT transform, the
wire format, or the token verifier — every one of which parses input this
project treats as untrusted.

### S4-2 · The performance targets are arithmetic, not measurement · **Observed**

`docs/18` is honest about this: the 1M-cell gate asserts the per-cell *payload*
and the 60 fps gate is not asserted at all. There is no load test, no soak test,
and no measurement of the server under concurrent editors.

### S4-3 · No test opens a file this project did not write · **Observed**

One committed fixture, generated here. The oracle (P2-003) and package
validation (P1B-003) diff against LibreOffice, which is a genuine independent
check — but no corpus of real-world `.xlsx` files from Excel, Google Sheets or
Numbers is exercised. Fidelity is asserted against files whose shape we chose.

---

## Order

S1 first, and S1-1 before S1-2: a crash on untrusted input is reachable by
anyone who can send a file, and no amount of correct behaviour above it matters.

Then S2-1 and S2-2, which are the difference between a service that degrades and
one that dies. Then S3-2 and S3-4, which make the server deployable at all, then
S3-1 so the rest can be operated, then S3-3 for the scalability requirement.

S4-1 belongs beside S1-1: the fuzz target is what stops the next one of these.
