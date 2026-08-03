# Governance

OpenCalc is maintained under the CasualOffice GitHub organization, alongside its
sibling document engine OpenDoc.

## Roles

**Maintainers** own repository administration, releases, security response, and
final compatibility decisions.

**Subsystem owners** review changes in their documented area (e.g. the package
reader, the workbook model, the calc engine, the grid layout/render, a format
adapter) and maintain its design, tests, fixtures, and tracker state.

**Contributors** may propose designs and changes through the documented
contribution process ([CONTRIBUTING.md](CONTRIBUTING.md)).

Named maintainers and subsystem owners will be recorded before the first public
preview. Until then, repository write access is the authoritative maintainer
signal.

## Decision Process

Substantial decisions follow the design-first process
([docs/11-DESIGN-FIRST-PROCESS.md](docs/11-DESIGN-FIRST-PROCESS.md)):

1. define the required outcome and constraints;
2. record research and alternatives;
3. publish a design note or ADR;
4. discuss and resolve objections;
5. mark the decision accepted;
6. update the tracker ([docs/14-EXECUTION-TRACKER.md](docs/14-EXECUTION-TRACKER.md));
7. implement and verify.

Maintainers seek technical consensus. When consensus is not available, the
maintainer responsible for the affected compatibility boundary records the
decision and its consequences in an ADR ([docs/08-ADR-REGISTER.md](docs/08-ADR-REGISTER.md)).

## Protected Areas

These require maintainer review because a change to them is expensive to reverse:

- public APIs and the SDK surface;
- the normalized workbook schema and the operation/transaction schema;
- **the crate boundaries / layer division** (the dependency DAG in
  [docs/19-WORKSPACE-SCAFFOLD-DESIGN.md](docs/19-WORKSPACE-SCAFFOLD-DESIGN.md));
- **the reserved calc seams** and the dependency-graph / recalculation model;
- parser and security policy, and any `unsafe` code;
- the display-list contract and render backends;
- SpreadsheetML preservation / round-trip behavior;
- the performance-target budgets ([docs/30-PERFORMANCE-AND-CAPACITY-TARGETS.md](docs/30-PERFORMANCE-AND-CAPACITY-TARGETS.md));
- release automation.

## Releases

Releases require all gates in
[docs/15-CI-AND-RELEASE-GATES.md](docs/15-CI-AND-RELEASE-GATES.md), an updated
changelog and tracker, compatibility notes, and reproducible artifacts. No single
contributor may silently weaken a release gate to publish.
