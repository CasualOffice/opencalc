# Security Policy

## Reporting

Do not open a public issue for a suspected vulnerability, malicious fixture, or
confidential workbook.

Use [GitHub private vulnerability reporting](https://github.com/CasualOffice/opencalc/security/advisories/new)
for this repository. Include:

- affected revision or release;
- affected subsystem and host mode (Tauri desktop / web WASM / headless);
- impact and realistic attack path;
- minimal reproduction without confidential content;
- whether active exploitation is known;
- suggested mitigation when available.

The project will acknowledge a complete report, assess severity, coordinate a
fix and advisory, and credit the reporter when requested. Do not publish details
before coordinated disclosure.

## Supported Versions

OpenCalc has no stable release yet — it is **alpha**, and nothing is published
to crates.io or npm. Security fixes target `main`. Supported release lines and
end-of-support dates will be listed here before the first public preview.

Two things are worth stating plainly while that is true. The engine already
admits untrusted workbooks under the bounds below, and those bounds are gated
and fuzzed — a parser bug is in scope for this policy today. But an integrator
running an alpha vendored from `webapp/` has no upgrade channel from us: there
is no auto-update and nothing phones home, so acting on an advisory is
manual until the packages are published.

## Security Boundaries

- workbooks, packages, XML, shared strings, styles, formulas, images, fonts, and
  operation logs are **untrusted input**;
- network access and external-reference / linked-resource fetching are **denied
  by default**;
- **macros / VBA are never executed** — VBA parts are preserved as opaque bytes,
  never run;
- **formula evaluation is bounded** — dependency-chain depth, iterative-calc
  iterations, and spill-region size are capped, and full recalculation is
  cancellable;
- parser and runtime limits are **required behavior**, not best-effort;
- normal diagnostics exclude workbook cell content and secrets.

See [`docs/07-QUALITY-SECURITY-AND-COMPATIBILITY.md`](docs/07-QUALITY-SECURITY-AND-COMPATIBILITY.md)
and [`docs/21-PARSER-LIMITS.md`](docs/21-PARSER-LIMITS.md) for the current threat
and resource policy, and [`docs/20-ERROR-CODE-REGISTRY.md`](docs/20-ERROR-CODE-REGISTRY.md)
for the diagnostic codes a bounded rejection returns.
