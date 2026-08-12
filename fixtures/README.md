# Fixtures

Test workbooks for OpenCalc, checksummed in [`manifest.json`](manifest.json) and
verified in CI (`repository-policy`). See
[`docs/29-PHASE-0-PLAN.md`](../docs/29-PHASE-0-PLAN.md).

## Layout

- `generated/` — synthetic fixtures produced by `tools/generate.py`,
  deterministically (fixed ZIP timestamps) so their SHA-256 is stable.
- `corpus/` — rights-reviewed real-producer `.xlsx` files. Anything added here
  must clear a rights review; record provenance in `manifest.json`.
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

## The corpus, and its rights review

Every other fixture here was written by this project, which makes the fidelity
tests circular in the way that matters: they prove the importer agrees with the
exporter, and say nothing about whether either agrees with Excel.

`corpus/` holds files **produced by Microsoft Excel**, taken from
[Apache POI's test data](https://github.com/apache/poi/tree/trunk/test-data/spreadsheet).

- **Licence:** Apache-2.0, which permits redistribution with attribution. POI
  ships these as test data under the project's own licence.
- **Provenance:** each entry in `manifest.json` records its `source` URL,
  `license` and `producer`, alongside the checksum CI already verifies. That is
  the rights review, written down rather than remembered.
- **Why these:** they contain what real documents contain and hand-written
  fixtures do not — a chart, a classic header, an image-only workbook, defined
  names, and parts this project has never emitted.

Adding more means the same three fields. A file whose licence cannot be
established does not go in, however useful it looks: the checksum makes the
bytes reproducible, and the provenance makes them defensible.
