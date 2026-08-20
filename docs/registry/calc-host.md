# OpenCalc document host

Stores spreadsheets, serves the editor, and hands out the signed tokens the
collaboration server checks. This is the piece that makes `docker run` a whole
product rather than a component.

**Alpha.** It is also, by its own description, the *demo* integrator: a real
deployment usually keeps documents in its own system and talks to
`casualoffice/calc` directly, or installs OpenCalc into Nextcloud or SharePoint
through `casualoffice/wopi`. It is published because a first five minutes that
needs a source checkout is a worse first five minutes.

## Run the whole thing

```sh
docker run -p 8080:8080 \
  -e OPENCALC_SHARED_SECRET="a secret of at least 16 bytes" \
  -v opencalc-documents:/data \
  casualoffice/calc-host
```

That gives you storage, the editor, upload and download, and version history.
For co-editing, add `casualoffice/calc` and point this at it — the compose file
in the repository wires all of it, including the proxy that puts the editor and
the collaboration socket on **one origin**, which browsers require.

## What it does

Upload, create, open, save, download, and keep versions. Documents are written
**atomically** — beside the target and renamed — so a crash mid-save leaves the
previous version rather than half a file. Formats: `.xlsx`, `.ods`, `.csv`,
`.tsv`, `.psv`, each saved back in the format it arrived as.

## Settings worth knowing

| | |
| --- | --- |
| `OPENCALC_SHARED_SECRET` | Signs the tokens the collaboration server checks. At least 16 bytes. |
| `OPENCALC_COLLAB_URL` | Where the collaboration server is. Omitted, documents open single-user. |
| `OPENCALC_DATA` | Where documents live. Mount it, or they go when the container does. |

Full list: `docs/65-RUNNING-IT.md` in the repository.

## Tags

`X.Y.Z` is immutable. `latest` moves, and never to a prerelease. Built for
`linux/amd64` and `linux/arm64`, with a build provenance attestation naming the
repository, workflow and commit that produced them.

## Source and licence

<https://github.com/CasualOffice/opencalc> — Apache-2.0.
