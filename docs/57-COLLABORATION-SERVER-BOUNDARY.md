# 57 — The Collaboration Server: what it owns, and what it must not

**Status: Proposed** (ADR-012). Follows [ADR-011](56-COLLABORATION-CONCURRENCY-DESIGN.md),
which settled *how* concurrent edits reconcile. This settles *where the thing
that does it runs*, and what else it is allowed to know about.

> **Decision.** A collaboration server that is **an editing session, not a
> filing system**. It coordinates co-editing, and carries the concerns that
> only exist once a deployment exists — auth, admin, telemetry, configuration.
> The document of record stays with the integrator. The server hands finished
> bytes back through a **webhook callback**; it never becomes the place the
> file lives.

## Two prior arts, and only one of them fits

The shape asked for was "the OnlyOffice path, or you could say the Excalidraw
path". Those are different systems, and the difference decides this document.

| | [Excalidraw](https://plus.excalidraw.com/blog/building-excalidraw-p2p-collaboration-feature) | [OnlyOffice](https://api.onlyoffice.com/docs/docs-api/get-started/how-it-works/saving-file/) |
| --- | --- | --- |
| Server role | relays **encrypted** messages between peers, "does no centralized coordination" | holds the document for the session, coordinates, compiles the result |
| Server sees the content | **no** — ciphertext only | yes |
| Storage | nothing server-side at all | nothing durable: the host downloads the finished file through `callbackUrl` |
| Reconciliation | client-side, last-writer-wins | server-ordered |

**The Excalidraw path cannot carry ADR-011.** Server-mediated operational
transform requires the server to *be the order* — to transform each incoming
operation against everything committed since its base. A relay that sees only
ciphertext cannot transform anything; that is precisely why Excalidraw
reconciles on the client instead. Choosing it would mean reopening the CRDT
decision, not deploying the one we made.

**The OnlyOffice path is what we have already built.** `ServerSession` plus
`Snapshot` *is* a document server: an ordering authority that holds the model
for a session's life and can assemble the file at the end. Nothing new is
needed to adopt it.

So: OnlyOffice's **shape**, with its storage model — the host owns the file,
the server hands it back — which is the part the request was really about.

## What the server owns

- **The order.** The single serialization point per document, per ADR-011.
- **The session's state**: the model, the revision log, the snapshots. Durable
  enough to survive a hibernation, not durable enough to be a system of record.
- **Authentication and authorization** — enforced, not decided. See below.
- **Admin, telemetry, configuration.** These exist only in a deployment, which
  is exactly why they live here and not in the SDK
  ([55](55-SDK-EMBEDDING-AND-INTEGRATION-DESIGN.md): the embeddable element
  ships with none, and that stays true).

## What the server must not own

- **The document of record.** It is not a file store, a version history, or a
  backup. It holds a working copy for as long as people are editing it.
- **Who may open what.** The host mints a signed token naming the document, the
  user, and their permission; the server verifies and enforces it. A server
  that decided access would need the host's whole permission model, and would
  be wrong about it.
- **Rendering, conversion, or a headless export service.** The engine can do
  those; bundling them here turns a session coordinator into a product with
  four jobs.

## The lifecycle

1. **Join.** The client presents a host-signed token. The server, on the first
   join, fetches the document from the URL the token names.
2. **Edit.** Ordinary ADR-011 traffic: operations in, transformed operations
   out, revisions counted, snapshots on the cadence from
   [`SnapshotPolicy`](../crates/casual-calc-transaction).
3. **Save points.** The server assembles `.xlsx` with the same engine — native
   Rust, the same writer the desktop uses — and **POSTs it to the host's
   callback**. On quiesce, on a revision interval, and when the last
   participant leaves.
4. **Idle.** State persists; compute may hibernate. Waking is a snapshot read.
5. **Close.** Final callback, then the session's state may be discarded.

### The window where work can be lost

Between save points. That is inherent to the host owning storage, and it is
worth stating rather than discovering: a server lost between callbacks costs
whatever was edited since the last one. The cadence is therefore a durability
setting, not only a performance one, and the default should be aggressive —
quiesce is a save point precisely because "everyone stopped typing" is the
cheapest moment to make the last minutes safe.

## The fidelity trap this design walks into

**A session must start from the original file, not from a model snapshot.**

[ADR-007](08-ADR-REGISTER.md) makes retained bytes authoritative for everything
the model does not represent — an unrecognised chart, a VBA part, `customXml`,
a drawing with shapes we do not model. That side table is built by *importing
the file*. A session that began from a model-only snapshot would be internally
consistent, converge perfectly, and write back a document with every preserved
part silently removed.

That is the exact failure the entire fidelity effort exists to prevent, and
collaboration is the one path that could reintroduce it — because it is the
only path where the document is reconstructed from something other than the
bytes it arrived as. So:

- the session's durable state is **the original bytes, the model snapshot, and
  the operation log** — not the snapshot alone;
- the callback assembles from the retention path exactly as a single-user save
  does;
- a session that cannot obtain the original bytes **refuses to start** rather
  than proceeding with a lossy copy.

## Webhooks now, WOPI later — and not foreclosed

Webhooks are perhaps a tenth of WOPI's surface and most of its value: a signed
POST carrying the finished document and the session's outcome. WOPI is a
protocol with locking, `CheckFileInfo`, `PutFile`, proof-key rotation and a
discovery document, and its real payoff is admission to the SharePoint and
Office ecosystems. That is a customer-driven reason, not an architectural one.

The design should therefore keep the seam where WOPI would attach — the
document is fetched through an interface, not a hardcoded HTTP call — so
adding it later is an implementation of that interface rather than a rewrite.

## Where the code lives

[ADR-003](08-ADR-REGISTER.md) fixes the crate DAG, so this needs saying
explicitly.

- **The protocol stays in the engine**, where it already is:
  `casual-calc-transaction::{transform, session}` is pure logic with no I/O,
  no clock and no transport, which is what let it be tested against thousands
  of interleavings without a socket.
- **The server is a host, not a layer.** AGENTS.md's rule — *the engine
  computes, the host decides I/O, network and persistence* — makes a network
  service the wrong thing to put inside the engine's dependency graph. It
  would drag an async runtime and an HTTP stack into a library that currently
  has neither.

Proposed: a workspace member under `server/`, alongside the existing `tools/`
convention, with one boundary rule — **nothing under `crates/` may depend on
it.** Versioned with the engine, excluded from the engine DAG, released on its
own `server-v*` tag ([15](15-CI-AND-RELEASE-GATES.md) §Release tags).

## Open questions

1. **Does the server ever serve the file to the client, or only the model?**
   Everyone in a session must start from the same revision, which argues the
   server hands out snapshot-plus-revision on join rather than letting each
   client fetch the file and start somewhere different.
2. **Save-point cadence defaults**, given the loss window above.
3. **Token shape.** JWT is the obvious choice and what OnlyOffice uses; what
   needs deciding is the claim set — document id, user id, permission, expiry,
   and whether the fetch URL is inside the token or resolved by the host.
4. **Presence identity.** The name and colour shown to other participants come
   from the token, not from the client, or a participant can claim to be
   someone else.
5. **What happens when the host's callback fails.** Retry with backoff is
   obvious; what is not is how long the server keeps trying before it has to
   tell the participants their work is not being saved.
