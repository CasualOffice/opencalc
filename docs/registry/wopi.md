# OpenCalc WOPI adapter

Makes OpenCalc installable into Nextcloud, ownCloud, SharePoint, Moodle and
Alfresco. Those five do not each have an integration API — they have this one,
and an administrator pastes a single discovery URL into a settings page.

**Alpha.**

## Run it

```sh
docker run -p 8090:8090 \
  -e OPENCALC_WOPI_PUBLIC_URL="https://calc.example.com" \
  casualoffice/wopi
```

Then point your host at `/hosting/discovery`.

## Why this is a separate image

It is not an arbitrary split. The collaboration server **cannot mint tokens and
holds no per-document state** — that is what lets exactly one node own a
document and the cluster grow by adding nodes. WOPI needs both a minting key
and a per-file lock. Folding it in would trade that property away.

Collabora and ONLYOFFICE ship one image because their daemon genuinely is both.
This project chose otherwise, deliberately, and the two images are that choice.

## Formats

`.xlsx` and `.ods` open and save back in kind, as do `.csv`, `.tsv` and `.psv`.
What a format cannot carry is **counted and named** in a compatibility report
rather than dropped quietly — the `.ods` writer keeps values, formulas and
sheets, and says what it left behind.

## Source and licence

<https://github.com/CasualOffice/opencalc> — Apache-2.0.
