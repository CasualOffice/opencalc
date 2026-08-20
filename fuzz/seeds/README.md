# Fuzzing seeds

Starting points, committed. `fuzz/corpus/` is where libFuzzer accumulates what
it discovers and is deliberately **not** committed — it is machine-specific,
large, and regenerable. These are the handful of inputs worth carrying, so a
fresh checkout and a CI runner begin from the same place a developer does.

Random bytes are a bad start for a structured format. A fuzzer given nothing
spends its budget being rejected at the first byte and never reaches the code
that reads an attribute or descends an element; given one real part to mutate,
it reaches all of it. Handing it something valid is what turns a fuzzer aimed at
a parser from a liveness check into a search.

Pass them alongside the corpus, **naming the corpus directory first** —
libFuzzer writes what it finds into the first directory it is given:

```sh
cargo +nightly fuzz run ooxml_xml corpus/ooxml_xml seeds/ooxml_xml
```

`ooxml_xml/` is every XML and `.rels` part of `fixtures/generated/minimal.xlsx`,
which is small, checksummed and produced by this project — so a seed changing is
a fixture changing, and both are visible in review.

`ods/` is a real LibreOffice `.ods` for the whole-package pass, that same
document's `content.xml` for the element-walk pass, and five small documents
aimed at what the reader gets wrong: repeat runs, escaped text, every value
type, the constructs that must be *reported* rather than dropped, and a
document with no sheets at all. `amplifier.xml` is a committed reproducer for
`ODS-03` — see the crash policy in [`../README.md`](../README.md).

`token_verify/` is a valid token plus the near misses worth starting adjacent
to: `alg: none`, a header claiming RS256 against a shared secret, a `kid`
carrying a path, a signature made with the wrong key, an expired token, and one
minted for a different document. A fuzzer will not discover the *shape* of a JWT
by mutating noise — three base64 segments and an HMAC over the first two — so
without these it would spend its whole budget failing to parse and prove
nothing. They are signed with the same secret the target hands the verifier,
which is the point: the question is not whether a fuzzer can be kept out, it is
whether anything it builds is **accepted**.

## Several producers, and why it matters

A corpus written by one program teaches a fuzzer that program's habits. Every
producer lays out the same format differently — attribute order, which parts
exist at all, whether a value is inline or shared, how a style table is
indexed — and a reader that only ever sees one of them has been tested against
one dialect of a format whose whole point is that anybody can write it.

`xlsx/` therefore does **not** carry copies of whole workbooks. It carries the
parts worth mutating, from four producers, and the run points at
`../fixtures/corpus` and `../fixtures/generated` directly for the packages —
those are rights-reviewed, recorded in `fixtures/manifest.json` with a
`producer` and a `license`, and checksummed by CI, so a seed cannot drift from
the fixture it came from and nothing is duplicated.

- `worksheet-{excel,libreoffice,openpyxl,xlsxwriter}.xml` and
  `styles-{excel,libreoffice,openpyxl}.xml` — the two largest readers, as four
  different programs write them.
- `worksheet-tables.xml` — a sheet carrying a structured-reference table.
- `libreoffice-export.xlsx` — a whole package written by
  `soffice --headless --convert-to xlsx` (LibreOffice 26.2.4.2) from
  `../ods/libreoffice-basic.ods`. LibreOffice's OOXML *export filter* is a
  producer the fixture corpus did not otherwise have.

`delimited/` covers what the typing rules turn on: quoting and embedded
newlines, leading zeros, ISO dates and times, booleans, non-ASCII text, a
sparse sheet, and the magnitudes at both ends of `f64`. `underflow.csv` is the
committed reproducer for the held defect — see the crash policy in
[`../README.md`](../README.md).

`snapshot/` is `to_snapshot` run over workbooks imported from four producers,
plus an empty workbook — the smallest thing the reader must still admit — and
`float-drift.json`, the held reproducer. A fuzzer will not discover the shape of
this format by mutating noise: it is a deep JSON object with interned handles
that have to resolve, and `validate` rejects anything whose handles do not.
