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

Pass them alongside the corpus:

```sh
cargo +nightly fuzz run ooxml_xml corpus/ooxml_xml seeds/ooxml_xml
```

`ooxml_xml/` is every XML and `.rels` part of `fixtures/generated/minimal.xlsx`,
which is small, checksummed and produced by this project — so a seed changing is
a fixture changing, and both are visible in review.
