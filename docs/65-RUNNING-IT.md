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

- **`OPENCALC_COLLAB_WS`** — what a **browser** dials. **Usually leave it
  unset.** The host derives it from the request the browser just made, which is
  the one address known to reach it: same origin, `/collab`, and `wss://` when
  `X-Forwarded-Proto` says the page arrived over TLS. Set it only when
  collaboration lives on a hostname of its own.
- **`OPENCALC_HOST_INTERNAL`** — what the **collaboration server** calls your
  host. On a compose network that is a service name. Point it at `localhost` and
  the server fetches itself.

They look interchangeable and are on different networks.

This used to default to `ws://127.0.0.1:8443/collab`, which reads like a working
setting and is the *browser's* own loopback. It worked for exactly one machine:
anybody opening a share link elsewhere dialled themselves and reconnected
forever, and an HTTPS page could not open `ws://` at all. Deriving it means the
demo does the thing it exists to demonstrate without being configured first.

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
then a single URL, with no CORS to configure and no second certificate. The
standalone stack does this itself: `docker compose up` starts an nginx in front
of both services ([`deploy/nginx.demo.conf`](../deploy/nginx.demo.conf)), so the
page and the socket share a hostname and the endpoint above needs no setting.

## Watching it

`GET /metrics` on the collaboration server, in Prometheus text format:

| Metric | Answers |
| --- | --- |
| `opencalc_saves_accepted_total` / `_failed_total` | Are documents getting back to the host? |
| `opencalc_save_duration_milliseconds_total` | Divided by the counts, how slow is your callback? |
| `opencalc_fetches_ok_total` / `_failed_total` | Can the server reach your host at all? |
| `opencalc_documents_unreadable_total` | Is the host answering 200 with something that is not a workbook? |
| `opencalc_revisions_total` | Is anything being edited? Counts **operations**, so it moves in step with the revision number rather than with submissions. |
| `opencalc_connections_refused_pending_total` | Is the node full while still answering `/healthz`? |
| `opencalc_joins_refused_capacity_total` | Are arrivals being turned away by a cap? |
| `opencalc_slow_consumers_total` | Are clients being dropped for lagging? Survivable, but silent. |
| `opencalc_appends_refused_total` | Cluster: is a fenced or stale leader still trying to write? |
| `opencalc_documents` / `opencalc_participants` | Current load, as gauges. |

The one to alert on first is `saves_failed_total` increasing: it is the only
counter that means work is at risk rather than merely that something is busy.

Every counter in this table is asserted to be incremented by some code path, by
a test that reads the fields off the `Metrics` struct itself
(`net::tests::every_counter_on_metrics_is_exposed_and_incremented_somewhere`).
Four of them once were not: they were declared, exposed, and written down here,
and reported zero for ever — which reads exactly like a quiet server, and is why
this list is checked against the code rather than maintained beside it.

`GET /stats` remains, returning the two gauges as JSON — it answers "is it
working *now*" for a person, where `/metrics` answers "has it been working" for
a machine.

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

## Putting it in Nextcloud, SharePoint or Moodle

Those file stores do not each have an integration API. They have one — WOPI —
and the way an editor gets into their list is a discovery document:

```
docker compose --profile wopi up -d
# then paste this into the file store's settings:
#   http://your-host:8090/hosting/discovery
```

Two settings have no useful default:

- `OPENCALC_WOPI_ALLOWED_HOSTS` — **required**, and the process refuses to start
  without it. The file's address arrives in a query string on a link, so an
  unrestricted adapter fetches whatever a link tells it to.
- `OPENCALC_WOPI_PUBLIC_URL` — the address a *browser* reaches the adapter on,
  which goes into the discovery document. Behind a proxy it is never the bind
  address.

`OPENCALC_BRAND_NAME` puts your own name on it — in that editor list, in the
browser tab, and in the editor's own toolbar.

Design and the full handshake: [74](74-WOPI-INTEGRATION.md).

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
- **Secrets belong in a file, not the environment.** Every secret has a
  `_FILE` form — `OPENCALC_SHARED_SECRET_FILE`, `OPENCALC_ADMIN_TOKEN_FILE`,
  `OPENCALC_REDIS_URL_FILE` — naming a path to read instead. An environment
  variable is readable in `docker inspect`, in `/proc/1/environ` by anything
  sharing the namespace, and in whatever a process manager logs; a signing key
  held that way leaks through four mechanisms nobody thinks of as a disclosure.
  Setting both forms is **refused**, not ranked, because a silent precedence
  rule is how a deployment that believes it moved to files keeps running on the
  variable it forgot to delete. A `_FILE` variable no server reads — the plural
  typo is the common one — is named in the log at startup, since a mount that is
  present, correct and ignored otherwise looks exactly like a missing secret.
- **One key set per tenant, or the issuer is only a label.** With
  `OPENCALC_ISSUERS=a=https://a/jwks.json,b=https://b/jwks.json` a token is
  checked against **the keys of the issuer it names**. Sharing one key set
  between tenants and checking `iss` against a policy does not work: the claim
  is filled in by whoever mints the token, so any tenant holding a trusted key
  can mint for any other tenant's documents. A single-tenant deployment can pin
  its issuer with `OPENCALC_ISSUER`, which refuses a token the same signer
  minted for somebody else — but cannot make one key set into a boundary.
- **A full node can point somewhere better, if you let it.** Set
  `OPENCALC_PUBLIC_URL` per node — `wss://node-2.example/collab`, the address a
  *browser* uses — and a node at its document cap answers with the least-loaded
  peer that has room rather than only refusing. It is deliberately not
  `OPENCALC_ADVERTISE`: that is a service name on the cluster network, which is
  exactly the address a client cannot reach. Leave it unset behind a single load
  balancer, where a redirect would return through the balancer to an arbitrary
  node. Watch `opencalc_joins_redirected_total` against
  `opencalc_joins_refused_capacity_total`: refusals that name nowhere mean the
  cluster is full, or that placement is silently doing nothing.
- **`OPENCALC_ALLOW_PLAIN_CALLBACKS=1`** lets the document travel in clear. It
  is off by default and on in the demo because the demo is loopback.
- **The host has no authentication.** Anybody who can reach it can open any
  document whose id they know. Your product supplies accounts; the demo does not
  pretend to.
- **Share links are bearer tokens.** Anyone with the link can edit, which is
  what the share dialog says.
- **The coordinator link is plaintext unless you say otherwise.** In a cluster,
  `OPENCALC_REDIS_URL` carries the lease that decides which node may write a
  document and every operation appended to the log. `redis://` sends all of it
  in clear, and the server says so once at startup. See below.

### The coordinator link, in a cluster

Only relevant when `OPENCALC_REDIS_URL` is set; standalone has no coordinator.

| Setting | What it is |
| --- | --- |
| `OPENCALC_REDIS_URL` | `redis://host:6379` for plaintext, **`rediss://host:6380` for TLS**. |
| `OPENCALC_REDIS_CA` | A PEM CA the coordinator's certificate must chain to, instead of the system trust store. This is the usual case: an internal Redis is not issued a certificate by a public authority. |
| `OPENCALC_REDIS_CLIENT_CERT` / `OPENCALC_REDIS_CLIENT_KEY` | This node's own certificate and key, when the coordinator requires one (`tls-auth-clients yes`). Both or neither. |
| `OPENCALC_REDIS_NAMESPACE` | The prefix every key and channel sits under. Change it when one Redis is shared between deployments — two sharing a prefix share leases, and a staging node will take leadership of a production document and be believed. |
| `OPENCALC_LEASE_MS` | How long a node's claim on a document lasts before it must be renewed (6000 by default). Longer means a node stays certain of ownership through a longer coordinator hiccup, and takes correspondingly longer to replace one that has genuinely died. |

Two shapes are **refused at startup** rather than started:

- certificates configured against a `redis://` URL, because that is a
  configuration that reads as encrypted and is not;
- `rediss://…/#insecure`, which encrypts the link to whoever answers the port
  rather than to your coordinator. Point `OPENCALC_REDIS_CA` at the CA instead.

Redis is not replicated: it is still one box, and losing it stops *ordering*
cluster-wide for as long as it is away, which clients are told about
(`NotSaving`) and which takes the node out of the pool (`/readyz` answers 503).
It comes back on its own when Redis does — nodes re-dial and re-claim, with no
restart needed. Replication and failover are designed in
[77](77-COORDINATOR-AVAILABILITY.md) and not built.

## Backing it up, and what you get back

**The recovery point is your backup interval, and nothing shortens it.** There
is no continuous archive and no point-in-time recovery: whatever was edited
between your last copy and the failure is gone. Say that number out loud before
choosing an interval, because it is the whole guarantee.

### What a document is

Three things, in the store directory:

    <id>.xlsx        the document
    <id>.json        its metadata — title, timestamps
    <id>.versions/   previous versions, one file each, named for the moment

They are written **one at a time**, document first, so an interruption leaves an
*unlisted* document rather than a listed one whose bytes are gone. Each
individual write is atomic — written beside the target and renamed — so no
single file is ever half-written (`DEP-12`).

### The thing to understand about a live copy

> A backup of a running store is a set of atomic files, not an atomic set of
> files.

Copy the volume while somebody is uploading and the copy can hold the document
without its metadata, or the metadata without the document. Neither is
corruption and both are recoverable — but a restore of that copy looks complete
and is quietly short.

**Two options, and the first is free:**

- **Stop the host, copy, start it.** A few seconds of downtime buys a copy with
  nothing in flight. For most deployments this is the right answer.
- **Copy live and check afterwards.** The host scans its store at startup and
  says what does not line up:

      WARN store is consistent
      WARN the store does not line up  invisible=["k3f9"] dangling=[] …

  `invisible` is a document nothing lists — its bytes are there and it will not
  appear. `dangling` is an entry that lists a document whose bytes are gone.
  `unfinished` is a `.part` from a write interrupted before its rename.
  `stranded` is a versions directory whose document is gone.

  It **reports and does not repair**, deliberately: an invisible document may be
  somebody's only copy and a dangling entry may be the record of one that should
  be hunted down. Those want opposite treatment, and only you know which.

### Restoring

Stop the host, replace the store directory, start it, and **read the first
warning line**. A restore that says `store is consistent` is complete. One that
does not has told you exactly which ids to look at.

## Data

Documents live in a named volume. `docker compose down` keeps it; `down -v`
deletes it.

The collaboration server holds the ordered document while people are in it and
hands the bytes back to the host's callback when they quiesce. Redis, in the
cluster, holds leases and the operation log for documents currently open — not
the documents themselves, which is why it runs with persistence off. Losing
Redis costs in-flight coordination, which nodes recover by re-claiming; it does
not cost anybody's spreadsheet.

That last sentence was not true until `DEP-13`, and the reason is worth
recording: the connection to Redis never re-dialled, so a node that lost it
never got it back and refused every edit for the rest of its life. "Recovered by
re-claiming" described the design and not the code. It re-dials now.
