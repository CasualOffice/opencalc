# Benchmarks

Reproducible micro-benchmarks for OpenCalc, emitted as versioned JSON by
`tools/casual-calc-benchmark`. See
[`docs/29-PHASE-0-PLAN.md`](../docs/29-PHASE-0-PLAN.md) and
[`docs/15-CI-AND-RELEASE-GATES.md`](../docs/15-CI-AND-RELEASE-GATES.md).

## Running

```sh
# Full run (200 iterations), release build recommended for representative timings:
cargo run --release -p casual-calc-benchmark -- --env <label>

# Smoke run (few iterations) — CI validates the report SHAPE with jq, not timings:
cargo run -p casual-calc-benchmark -- --smoke --env ci
```

The report is printed to stdout. Each case reports `medianNs`, `p95Ns`, an
`outputChecksum` (identical across iterations proves the operation is
deterministic), `deterministic`, and a `maxRegressionBasisPoints` tolerance.

## Cases (current)

| id | Operation |
| --- | --- |
| `model-snapshot-roundtrip-10k` | `Workbook` with 10k cells → snapshot → reopen → snapshot |
| `package-open-small` | Admit a small OPC package and read a part |

Cases grow as the engine does; the 1M-cell / 60 fps / <50 ms-recalc target
benchmarks ([docs/30](../docs/30-PERFORMANCE-AND-CAPACITY-TARGETS.md)) are added
with the layout and calc phases.

## Baselines

`baselines/<label>.json` holds a committed reference run for a named environment.
Baselines are **reviewed artifacts**: update one only as an intentional,
reviewed source change, never incidentally. The environment label carries no PII.
`baselines/dev-reference.json` is an indicative developer baseline, not a CI gate;
CI's `benchmark-smoke` job validates report shape only.
