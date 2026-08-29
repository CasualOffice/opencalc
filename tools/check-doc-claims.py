#!/usr/bin/env python3
"""No document claims a gate that CI does not run.

`DOC-025`'s acceptance is exactly that sentence, and until now nothing enforced
it — `67` stated DOC-025's own gate as "a documentation consistency check
rejects the known contradictory phrases", and no such check existed. The row
that exists to catch unbacked gate claims was itself one (`DOC-032`).

That is the failure this guards: a document promising a check nobody wrote is
worse than a document promising nothing, because it is read as assurance. The
same applies to a *job* name — `15` publishes the CI job list as a contract, and
a job named there and absent from the workflow is a gate that cannot fail.

Three things are checked, all mechanical and none a judgement call:

  1. every `tools/check-*.py` a document names exists **and** is run by CI
  2. every CI job name a document names exists in the workflows
  3. every job the PR workflow runs is **named in `15`'s table**

Rule 3 is the same contract read backwards, and it was the half that was
missing. `15` calls its job list "a contract: they are stable, and a PR is not
mergeable until they pass", and a contract is only one if it is complete. Three
jobs — `sdk-types`, `docker-build` and `desktop` — were added to `ci.yml` and
never to the table, so the doc published "twelve jobs" against fifteen and an
integrator reading the contract could not see the gate their pull request would
actually fail. Nothing catches that from the doc side: rules 1 and 2 only ask
whether what the doc *says* is true, and a doc that says less is never wrong.

What is deliberately NOT checked: prose claims with no identifier in them. A
sentence like "this is covered by a test" cannot be verified by grep, and a gate
that pretended to would be the very thing it is meant to prevent. Nor are the
release and scheduled workflows held to rule 3 — `15`'s table is the **PR**
contract, and `release-images`, `release-sdk` and `security.yml` are gated by
`check-release-hold.py` and named in prose instead.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(".")
WORKFLOWS = sorted((ROOT / ".github" / "workflows").glob("*.yml"))
DOCS = sorted(ROOT.glob("docs/*.md")) + [
    p for p in (ROOT / "AGENTS.md", ROOT / "CLAUDE.md", ROOT / "README.md") if p.exists()
]

SCRIPT = re.compile(r"tools/(check-[a-z0-9-]+\.py)")

# The workflow whose jobs `15` publishes as the PR contract, and the document
# that publishes them.
PR_WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
GATES_DOC = ROOT / "docs" / "15-CI-AND-RELEASE-GATES.md"


def workflow_text():
    return "\n".join(p.read_text() for p in WORKFLOWS)


def job_names():
    """Top-level job keys across every workflow."""
    names = set()
    for p in WORKFLOWS:
        body = p.read_text().split("\njobs:\n", 1)
        if len(body) < 2:
            continue
        names |= {m.group(1) for m in re.finditer(r"^  ([a-z][a-z0-9-]*):$", body[1], re.M)}
    return names


def main():
    if not WORKFLOWS:
        print("no workflows found", file=sys.stderr)
        return 1

    ci = workflow_text()
    jobs = job_names()
    problems = []

    for doc in DOCS:
        text = doc.read_text()
        for script in sorted(set(SCRIPT.findall(text))):
            if not (ROOT / "tools" / script).exists():
                problems.append(f"{doc}: names tools/{script}, which does not exist")
            elif f"tools/{script}" not in ci:
                problems.append(f"{doc}: names tools/{script}, which no workflow runs")

        # A job name is only a claim when the document says it is one, so this
        # looks for the backticked name in a sentence about CI rather than for
        # the bare word — otherwise every occurrence of "test" is a finding.
        for m in re.finditer(r"`([a-z][a-z0-9-]{3,})`\s+(?:job|gate)\b", text):
            name = m.group(1)
            if name not in jobs:
                problems.append(f"{doc}: calls `{name}` a CI job, and no workflow defines one")

    # Rule 3: the contract is complete.
    contract = []
    if PR_WORKFLOW.exists() and GATES_DOC.exists():
        body = PR_WORKFLOW.read_text().split("\njobs:\n", 1)
        contract = re.findall(r"^  ([a-z][a-z0-9-]*):$", body[1], re.M) if len(body) > 1 else []
        # **The table, not the prose.** A first version looked for the
        # backticked name anywhere in the document and passed on a paragraph
        # that mentioned `docker-build` while *saying it was missing from the
        # table* — which is the gate agreeing with the defect. The contract is
        # the row: `| \`name\` | command | enforces |`.
        published = {
            m.group(1)
            for m in re.finditer(r"^\|\s*`([a-z][a-z0-9-]*)`\s*\|", GATES_DOC.read_text(), re.M)
        }
        for name in sorted(set(contract)):
            if name not in published:
                problems.append(
                    f"{GATES_DOC}: does not name the `{name}` job, which "
                    f"{PR_WORKFLOW} runs on every push — the job list there is a "
                    f"contract, and a contract that silently gains clauses is not one"
                )

    if problems:
        print("documents claiming gates that CI does not run:", file=sys.stderr)
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        print(
            "\nEither wire the gate up, or change the document — but do not "
            "quietly drop the promise: if the gate should exist, it is a row.",
            file=sys.stderr,
        )
        return 1

    print(
        f"doc claims: {len(DOCS)} documents, {len(jobs)} CI jobs, every named gate is real "
        f"and all {len(set(contract))} PR jobs are in the contract"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
