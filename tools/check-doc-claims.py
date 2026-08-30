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

    # **A document naming the protocol version must name the current one.**
    #
    # `AGENTS.md` said `PROTOCOL_VERSION` 5 while the code said 7 — it had
    # drifted by two bumps with nothing to notice. That number is not
    # decoration: it is what an old client is refused on, so a document stating
    # the wrong one tells a reader the wire is compatible when it is not.
    #
    # The version itself is read from the source rather than listed here, so
    # this gate cannot be the thing that goes stale.
    proto_src = (ROOT / "crates/casual-calc-transaction/src/protocol.rs").read_text(encoding="utf-8")
    found = re.search(r"PROTOCOL_VERSION: u32 = (\d+)", proto_src)
    if not found:
        problems.append(
            "could not read PROTOCOL_VERSION from protocol.rs — this gate reads it "
            "from the source on purpose, so a rename here is a gate that has stopped checking"
        )
    else:
        current = found.group(1)
        # The trackers are a *record*: rows say "PROTOCOL_VERSION 6 -> 7"
        # because that is what happened, and rewriting a record to match today
        # falsifies it. They are exempt for the same reason
        # `check-doc-references` exempts the archive.
        HISTORICAL = {"14-EXECUTION-TRACKER.md", "14a-ARCHIVE-CLOSED-WORK.md"}
        for doc in DOCS:
            if doc.name in HISTORICAL:
                continue
            text = doc.read_text(encoding="utf-8")
            # Present tense only. "`PROTOCOL_VERSION` 7" and "PROTOCOL_VERSION
            # is 7" are claims about now; "6 -> 7" and "moves to 8" are not.
            # **One intervening word used to defeat this.** The pattern was
            # `PROTOCOL_VERSION` then optionally `is` then the number, so
            # "is 5" was caught and "is *at* 5" sailed past — and `docs/08`
            # sat two bumps stale behind exactly that. A gate a synonym can
            # walk around is not checking the claim, it is checking a phrasing.
            #
            # So: the first number within a short window, whatever prose sits
            # between, and the window is short so a claim cannot reach into an
            # unrelated figure in the next sentence.
            #
            # A **transition** is not a claim about now — "6 -> 7", "moves to
            # 8", "goes to 2 ... it has since moved on" are records of a change
            # and are correct as written. `docs/61` documents the bump that ADR
            # made and would otherwise be reported for stating history.
            # **This cannot tell an asserted number from a rejected one.**
            # `docs/84` carried a sentence saying the tempting conclusion that
            # the version could stay where it was is *wrong* — correct prose,
            # read here as a stale claim. Excluding negations with a regex over
            # one line is not possible, so the document was reworded to name the
            # transition instead. If this fires on a sentence that is right,
            # prefer rewording the sentence over widening this pattern: the
            # version number is what an old client is refused on, and a gate
            # that learns to ignore more spellings of it will miss the real one.
            TRANSITION = re.compile(
                r"->|→|moves?\s+to|goes\s+to|bumped?\s+to|moved\s+on|raise[sd]?\s+to"
            )
            for m in re.finditer(r"`?PROTOCOL_VERSION`?[^\n\d]{0,24}?(\d+)", text):
                line_start = text.rfind("\n", 0, m.start()) + 1
                line_end = text.find("\n", m.end())
                line = text[line_start : line_end if line_end != -1 else len(text)]
                if TRANSITION.search(line):
                    continue
                if m.group(1) != current:
                    problems.append(
                        f"{doc.name}: says `PROTOCOL_VERSION` {m.group(1)}, but it is {current}. "
                        f"That number is what an old client is refused on, so a document "
                        f"stating the wrong one says the wire is compatible when it is not"
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
