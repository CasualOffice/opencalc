# OpenCalc collaboration server

The server that lets several people edit one spreadsheet at once. It orders
operations, relays them, and hands the document back to whoever is storing it.

**Alpha.** The engine, editor, SDK and collaboration are live and exercised by
two real browsers on every push. Operationally it is younger than that: there
is no Helm chart yet, and Redis is a single node.

## Run it

```sh
docker run -p 8443:8443 \
  -e OPENCALC_SHARED_SECRET="a secret of at least 16 bytes" \
  -e OPENCALC_ALLOWED_HOSTS="files.example.com" \
  casualoffice/calc
```

It is a *collaboration* server, not a file store: it fetches each document from
your own host over a URL carried in a signed token, and saves it back the same
way. `OPENCALC_ALLOWED_HOSTS` is the list of hosts it may fetch from — compared
**with the port stripped**, so `files.example.com`, not `files.example.com:443`.

For a whole stack — editor, storage, proxy — see the compose file in the
repository.

## What it costs

Measured, not estimated: an idle node is about 6.6 MB, and each open document
costs roughly **190 KB** on top of its cells at three editors. Cells are about
**84 bytes** each. So:

```
RAM ≈ documents × (cells × 84 bytes + 190 KB) × 1.2
```

At 150 concurrent editors across 50 documents: 15.9 MB resident, every edit
acknowledged, p50 18 µs. The 190 KB matters more than it looks — for a
1 000-cell sheet it is **70%** of the cost, so a deployment of many small
documents is sized by document count, not by cell count.

There is **no cap** on documents or people. A node admits work until its memory
is short, and stops at 85% of its container's limit — reaching the ceiling is
an OOM kill, and that takes every document on the node including unsaved work.

## Settings worth knowing

| | |
| --- | --- |
| `OPENCALC_SHARED_SECRET` | Signing key. At least 16 bytes. Prefer `OPENCALC_JWKS_URL`: a shared secret lets this process *mint* tokens as well as check them. |
| `OPENCALC_ALLOWED_HOSTS` | Hosts it may fetch documents from. Empty means any, which you do not want. |
| `OPENCALC_REDIS_URL` | Coordination for a cluster. Omitted, the node runs standalone. |
| `OPENCALC_MEMORY_HIGH_WATER_PERCENT` | Where admission stops. 50–95, default 85. |

Full list: `docs/65-RUNNING-IT.md` in the repository.

## Tags

`X.Y.Z` is immutable. `latest` moves, and never to a prerelease. Images are
built for `linux/amd64` and `linux/arm64`, and carry a build provenance
attestation naming the repository, workflow and commit that produced them.

## Source and licence

<https://github.com/CasualOffice/opencalc> — Apache-2.0.
