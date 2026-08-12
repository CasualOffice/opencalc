# Running OpenCalc

Two stacks, both `docker compose up`. Standalone is one collaboration node and
is what most deployments should run. The cluster is two nodes behind a proxy
coordinating through Redis, and buys horizontal capacity rather than
correctness — ADR-012 is explicit that standalone is a first-class mode and not
a degraded one.

## Try it in two minutes

```sh
cp .env.example .env       # then change OPENCALC_SHARED_SECRET
docker compose up --build
open http://localhost:8080
```

New spreadsheet → **Share** → send the link to somebody → you are both editing
the same sheet. **Download** takes away an `.xlsx` containing what you both
typed.

The cluster is the same thing with more of it:

```sh
docker compose -f docker-compose.cluster.yml up --build
open http://localhost:8090
```

## What the containers are, and why there are two

| | what it is | who supplies it in a real deployment |
| --- | --- | --- |
| **host** | documents, identity, tokens, saving | **you** — this is your product |
| **collab** | ordering, presence, the WebSocket | OpenCalc |

That split is not an artefact of the demo; it is the integration boundary. The
collaboration server holds **no per-document state** and deliberately **cannot
mint tokens**. It is told, per join, by a party that already knows: where the
file lives, where the finished bytes go, who is joining and what they may do.

`server/casual-calc-host` is that party, written small enough to read in one
sitting. When you integrate, you replace it. The collaboration server and the
editor stay as they are.

Four endpoints and a signature is the whole contract:

```
GET  /api/documents/{id}/content     what the collaboration server fetches
POST /api/documents/{id}/callback    where the finished bytes come back
GET  /api/documents/{id}/session     a minted token, for the browser
GET  /api/documents/{id}/download    what a user takes away
```

## The two addresses that get confused

This is the most common first failure, and it produces an editor that says
"connecting" forever with nothing in any log.

- **`OPENCALC_COLLAB_WS`** — what a **browser** dials. Behind a proxy this is
  your public URL with `wss://`.
- **`OPENCALC_HOST_INTERNAL`** — what the **collaboration server** calls your
  host. On a compose network that is a service name. Point it at `localhost` and
  the server fetches itself.

They look interchangeable and are on different networks.

## Behind a reverse proxy

Configurations for nginx and Caddy are in [`deploy/`](../deploy). Two things are
not obvious and both are marked in the files:

1. **The WebSocket upgrade headers.** Without `Upgrade` and `Connection`, the
   handshake is answered with a plain `200` and the editor reconnects forever.
2. **A read timeout longer than the server's client ping.** A co-editing socket
   is idle whenever nobody is typing, and nginx closes idle proxied connections
   after sixty seconds by default. The server should decide when a participant
   is gone; the proxy deciding for it looks like a network fault to everybody.

Serve everything from one origin — editor, API and WebSocket. A share link is
then a single URL, with no CORS to configure and no second certificate.

## Configuration

Everything is environment variables, read once at startup and checked before the
listener opens. A node that starts and is subtly wrong costs more than one that
refuses to start and says why.

`.env.example` documents what an operator actually changes. The rest is in
`main.rs` for each binary, which is the only copy that cannot go stale.

### Changing things while it is running

`/admin` — off entirely unless `OPENCALC_ADMIN_TOKEN` is set. There is no
default password.

It changes what can move under a live server: the endpoint new sessions are
given, whether uploads are offered, the banner. Settings are written to a file
beside the documents, so they survive a restart.

It will not change bind addresses, TLS, the signing secret, the Redis URL or a
node's identity, and it says so beside them rather than accepting the change and
ignoring it. A new secret invalidates every token in flight; a new bind address
is a different server; a node changing identity mid-lease is the zombie the
epoch fence exists to stop.

## Security, honestly

The demo defaults are demo defaults, and the ones that matter are:

- **`OPENCALC_SHARED_SECRET`** is symmetric, so the host and the server share a
  key and either could mint a token. A real deployment should use
  `OPENCALC_JWKS_URL` instead, where the server can verify a token and cannot
  make one. The server warns at startup when it is given a shared secret.
- **`OPENCALC_ALLOW_PLAIN_CALLBACKS=1`** lets the document travel in clear. It
  is off by default and on in the demo because the demo is loopback.
- **The host has no authentication.** Anybody who can reach it can open any
  document whose id they know. Your product supplies accounts; the demo does not
  pretend to.
- **Share links are bearer tokens.** Anyone with the link can edit, which is
  what the share dialog says.

## Data

Documents live in a named volume. `docker compose down` keeps it; `down -v`
deletes it.

The collaboration server holds the ordered document while people are in it and
hands the bytes back to the host's callback when they quiesce. Redis, in the
cluster, holds leases and the operation log for documents currently open — not
the documents themselves, which is why it runs with persistence off. Losing
Redis costs in-flight coordination, which nodes recover by re-claiming; it does
not cost anybody's spreadsheet.
