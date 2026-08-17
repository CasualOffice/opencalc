# 74 — WOPI integration

**Status:** design, being implemented. Tracker rows `WOPI-01`…`WOPI-04`,
`BRAND-01`.

## Why this and not another connector

Nextcloud, ownCloud, SharePoint, Moodle and Alfresco do not each have an
integration API for editors. They have *one*, and it is WOPI. Collabora Online
and ONLYOFFICE are installed into all five by implementing it. One protocol —
a discovery document and four request shapes — is the difference between
"OpenCalc can be embedded by someone who writes code against our SDK" and
"OpenCalc appears in the list of editors an administrator picks from".

Nothing else on the roadmap buys that much reach for that little surface.

## Which side we are on

WOPI's names are the opposite way round from the intuition, and getting them
backwards is the classic way to design the wrong thing:

- The **WOPI host** is the *storage*: Nextcloud, SharePoint. It serves the file.
- The **WOPI client** is the *editor*: Collabora, ONLYOFFICE — and us. It asks
  for the file and puts it back.

So OpenCalc implements the client. We serve a discovery document and an action
URL; we *call* `CheckFileInfo`, `GetFile`, `Lock` and `PutFile` on the host.

## Where it lives, and why not in either existing service

Neither of the two services we have is the right home, for opposite reasons.

`casual-calc-collab-server` **cannot mint tokens and holds no per-document
state** — ADR-012 and ADR-014, deliberately. WOPI needs both: an access token
per file, and a lock held for the life of an editing session. Putting them
there would undo the property that makes the server safe to scale.

`casual-calc-host` is the demo integrator, and says so in its own first
paragraph: not multi-tenant, not authenticated, documents in a directory. An
integrator inheriting a demo is exactly what that file warns against.

So WOPI is a **third service, `casual-calc-wopi`** — a WOPI client on one side
and an ordinary OpenCalc integrator on the other. That is the whole design:

```
Nextcloud  --discovery-->  wopi adapter  --mints a session token-->  browser
    ^                          |                                       |
    |  CheckFileInfo/GetFile   |                                       | WebSocket
    |  Lock/PutFile/Unlock     v                                       v
    +---------------------  adapter  <-----saves the package-----  collab server
```

The adapter is a real integrator, not a demo one, because WOPI answers the two
questions that made the demo host a demo: **who the user is** (the access
token, checked by the host) and **where documents live** (the host). There is
nothing left for us to hand-wave.

### The collab server does not change

This is the point of the shape. The adapter gives the collab server a
`Callback::Url` pointing back at *itself*, not a `Callback::Wopi` pointing at
the storage. Every WOPI-specific request — the override headers, the lock id,
the 409 handling — happens in one process that already holds the WOPI session.
The collab server keeps its single notion of a session and needs no new state.

`Callback::Wopi` stays: it is the right thing for an integrator who is already
a WOPI host themselves and wants the save to go direct. It is simply not the
path the adapter uses.

## The handshake

1. An administrator points their host at `https://opencalc.example/hosting/discovery`.
2. A user opens a spreadsheet. The host sends the browser to the action URL it
   found there, with `?WOPISrc=<file api url>&access_token=<opaque>`.
3. The adapter calls **`CheckFileInfo`** — `GET {WOPISrc}?access_token=…`. This
   both validates the token (the host rejects a bad one) and returns
   `BaseFileName`, `Size`, `Version`, `UserCanWrite`, `UserFriendlyName`.
4. If the file is writable and the host `SupportsLocks`, the adapter takes a
   **lock** and remembers its id for this session.
5. The adapter mints an OpenCalc session token — the ordinary `Claims`, signed
   with its own key — naming `{WOPISrc}/contents?access_token=…` as the fetch
   URL and itself as the callback, and serves the editor with it.
6. Editing is unchanged: the browser and the collab server do what they always
   do.
7. On save, the collab server POSTs the package to the adapter, which
   **`PutFile`**s it to the host bearing `X-WOPI-Lock`, then **unlocks** when
   the session ends.

Step 3 is what makes the token check honest: we never validate the access token
ourselves, because we cannot — it is the host's. We ask the host, by using it.

## Decisions

**The access token is never logged and never put in a URL we emit.** It is a
bearer credential for someone else's file store. It travels in query strings
because WOPI says so, which means it will be in the host's access logs and must
not also be in ours.

**Redirects are not followed on any WOPI call**, matching the existing fetch
path. A redirect would take a token somewhere the host never named.

**Read-only is a real mode, not a disabled toolbar.** `UserCanWrite: false`
mints a token without write permission, so the server refuses edits rather than
the browser hiding them.

**Locks are refreshed on a timer, not on activity.** WOPI locks expire after 30
minutes; the adapter refreshes every 10. Tying it to activity means a document
left open over lunch loses its lock and the save at the end of it.

## What this does not do

- **`PutRelativeFile`** (Save As) and `RenameFile`. Both are optional in WOPI
  and neither is needed to open, edit and save.
- **Proof keys.** WOPI's request-signing scheme is optional and no host
  requires it. It is the next thing to add for SharePoint Online hardening.
- **Anything about `.docx` or `.pptx`.** The discovery document advertises the
  spreadsheet formats this engine actually reads.
