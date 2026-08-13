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

## Fonts, and why a sheet might be a row of boxes

The editor draws every cell **the browser's way**, with the browser's own fonts,
so what a person types is drawn correctly in any script whether or not this is
configured. What is affected is the **headless renderer** — thumbnails, previews,
server-side PNG export.

That renderer carries **Latin only**, deliberately. Bundling coverage for every
script would put megabytes into every browser that opens the editor, for
languages most deployments never see, and it would make this project the arbiter
of which languages are worth carrying ([ADR-018](64-TEXT-SHAPING.md)). A
deployment knows which scripts its documents are in; this does not.

**Drop `.ttf`, `.otf` or `.ttc` files into the font directory.** The demo host
lists them at `/api/fonts` and its document page registers them, so for the
compose stacks that is the whole procedure:

```sh
mkdir -p ./fonts && cp NotoSansArabic-Regular.ttf ./fonts/
docker compose restart host
```

Integrating rather than running the demo? The editor **never probes for a font
service** — a host that does not run one would get a 404 in the console of every
session — so opt in by naming yours:

```
/editor/editor.html?fonts=/api/fonts
```

Bare `?fonts` means `/api/fonts`. Answer it with the URLs to fetch, which can
point anywhere — a CDN, a versioned path, another origin:

```json
{ "fonts": ["/fonts/NotoSansArabic-Regular.ttf"] }
```

Which face for which script — any face covering the script works, and the Noto
families are the usual answer because they are SIL Open Font Licence and cover
nearly everything:

| script | a face that covers it |
| --- | --- |
| Latin, Greek, Cyrillic, Hebrew | **already bundled** — nothing to do |
| Arabic, Persian, Urdu | Noto Sans Arabic |
| Devanagari (Hindi, Marathi) | Noto Sans Devanagari |
| Bengali, Gujarati, Tamil, Telugu, Malayalam | Noto Sans *(that script)* |
| Thai, Burmese | Noto Sans Thai / Myanmar |
| Chinese, Japanese, Korean | Noto Sans CJK *(pick the regional variant)* |
| emoji | Noto Color Emoji |

Faces are searched **before** the bundled ones and in filename order, so a
deployment that supplies a face gets it rather than a bundled near-match — and
the same document renders in the same face on every boot.

None is shipped here. A CJK face alone is tens of megabytes, and which one is a
regional decision this project should not be making for anybody.

### Finding out before a user does

A box is indistinguishable from a rendering bug, so ask instead of waiting:

- `/admin` lists the font directory and every face found in it.
- The SDK's `missing_font_coverage()` answers for a document — `[]` for almost
  all of them, and `[{ script: "Thai", sample: "ไ" }]` when there is a gap.
- In the browser, `missing_font_scripts(text)` answers the same question.

Each names the script and one character from it, which is the sentence worth
showing a user: *this sheet contains Thai and no face covering it is installed*.

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
