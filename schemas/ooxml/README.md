# Vendored OOXML schemas

The authoritative element and attribute inventory for the fidelity audit
([`tools/fidelity-audit`](../../tools/fidelity-audit),
[`docs/51`](../../docs/51-FIDELITY-GAP-AUDIT.md)).

These are here so the audit's "what *should* exist" side comes from the
standard rather than from anyone's recollection of it. Every construct missed so
far — tables, the 1904 epoch, the run of "cosmetic" P2 items that turned out to
be silent data loss — was missed because the checklist was written from memory.

## Provenance

| | |
| --- | --- |
| Source | `https://ecma-international.org/wp-content/uploads/ECMA-376-1_5th_edition_december_2016.zip` |
| Standard | ECMA-376 Part 1, 5th edition (December 2016), Ecma International |
| Archive within | `OfficeOpenXML-XMLSchema-Strict.zip` |
| Downloaded | 2026-08-09 |
| Archive SHA-256 | `9d0bcad9cf06054785b03762fcfadbf6bab7e54a5f9d69434e34b7fd464d4129` |

| File | SHA-256 |
| --- | --- |
| `sml.xsd` | `dc4b61faf2b62d2caf875faf40bb14a8f3c0cf3dc52763a2db394a28e79634b4` |
| `shared-commonSimpleTypes.xsd` | `02df434395eb22d0e13aa2f6445b7f7fbecd3687c6dff238823bbdd3bd9cdf6a` |
| `shared-relationshipReference.xsd` | `82409aacec09c8672eec2f8d7e44bec10c450649f31603659a8229b122a01163` |

**Rights review: outstanding.** These are third-party files under
[`fixtures/README.md`](../../fixtures/README.md)'s policy, which requires a
review before third-party material is committed. They are published openly by
Ecma International as part of a freely available standard, but that is not the
same as a review, and nobody has done one. Flagged rather than assumed.

## Known limitation: Strict, not Transitional

This is the **Strict** schema — note the `purl.oclc.org` target namespaces.
Real-world `.xlsx` files are almost always **Transitional**
(`schemas.openxmlformats.org`), which is a superset carrying deprecated
constructs Strict removed.

For an element/attribute inventory the two agree across the parts audited, so
the gap register is sound. It is nonetheless **conservative**: a
Transitional-only construct cannot appear in the inventory, so it cannot be
reported as a gap. Do not read the audit as proving there is nothing else.
Closing this needs the Part 4 Transitional schemas.
