# Fixtures

Test workbooks for OpenCalc, checksummed in [`manifest.json`](manifest.json) and
verified in CI (`repository-policy`). See
[`docs/29-PHASE-0-PLAN.md`](../docs/29-PHASE-0-PLAN.md).

## Layout

- `generated/` — synthetic fixtures produced by `tools/generate.py`,
  deterministically (fixed ZIP timestamps) so their SHA-256 is stable.
- `corpus/` *(later)* — rights-reviewed real-producer `.xlsx` files. Anything
  added here must clear a rights review; record provenance.
- `manifest.json` — `{ path, sha256, kind }` per fixture; CI rejects any fixture
  whose bytes don't match, and any unmanifested fixture.

## Regenerating

```sh
python3 fixtures/tools/generate.py
```

Regeneration must not change any committed SHA-256 (generation is deterministic).
If a checksum changes, that is an intentional, reviewed fixture change — update
the manifest in the same PR.

## Policy

- Synthetic fixtures are preferred; they carry no rights concerns.
- A real-producer file requires a rights review before it enters `corpus/`.
- Hostile inputs (zip bombs, path traversal, malformed XML) are currently
  exercised as in-crate tests that synthesize the bytes in memory
  (`casual-calc-package`, `casual-calc-ooxml`); committed hostile fixtures can be
  added here as the fuzz corpus grows.
