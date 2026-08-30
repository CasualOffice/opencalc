#!/usr/bin/env python3
"""A workflow step that can run on macOS may not use bash 4 features.

`REL-01`: the desktop release collected its Linux and Windows bundles and failed
on macOS with

    shopt: globstar: invalid shell option name

macOS ships **bash 3.2** — the last GPLv2 release — and has done for fifteen
years. `shopt -s globstar` is not a no-op there, it is an error, and under
`set -e` it takes the step with it. The `.dmg` was never collected.

What makes this worth a gate rather than a one-line fix is *where* it failed.
The same step already carried a hand-written portable `sum256()`, written
precisely because `sha256sum` is absent on macOS — so the platform difference
was known, thought about, and then walked into anyway two lines above. Knowing
the rule does not catch the next instance; a check does.

It is also invisible until a release: `ci.yml` builds a macOS `.app` and never
runs the release's collection step, so nothing on any pull request could fail.

**This resolves per job, not per file.** The first version of this gate matched
any file that mentioned a macOS runner anywhere and immediately produced two
false positives — `mapfile` in `ci.yml`, in two jobs that are `ubuntu-latest`
and always will be. A check that is wider than the claim it prints is the exact
failure this repository keeps meeting, so the matrix is resolved and only steps
that can actually land on macOS are read.
"""
import pathlib
import re
import sys

import yaml

BASH4 = [
    (re.compile(r"shopt\s+-s[^\n#]*\bglobstar\b"), "shopt -s globstar"),
    (re.compile(r"(?<![\w-])declare\s+-A(?![\w-])"), "declare -A (associative array)"),
    (re.compile(r"(?<![\w-])readarray(?![\w-])"), "readarray"),
    (re.compile(r"(?<![\w-])mapfile(?![\w-])"), "mapfile"),
    (re.compile(r"\$\{[A-Za-z_][A-Za-z0-9_]*\^\^"), "${var^^} (case conversion)"),
    (re.compile(r"\$\{[A-Za-z_][A-Za-z0-9_]*,,"), "${var,,} (case conversion)"),
]

MACOS = re.compile(r"macos-", re.I)


def runners_for(job: dict) -> list[str]:
    """Every runner label this job can land on, matrix included.

    `runs-on: ${{ matrix.os }}` names nothing by itself, so the matrix has to be
    resolved — that is the whole reason a release job is in scope at all.
    """
    raw = job.get("runs-on", "")
    labels = []
    if isinstance(raw, str):
        labels.append(raw)
    elif isinstance(raw, list):
        labels += [str(x) for x in raw]

    if any("matrix." in l for l in labels):
        matrix = (job.get("strategy") or {}).get("matrix") or {}
        for key, val in matrix.items():
            if key == "include":
                for entry in val or []:
                    labels += [str(v) for v in (entry or {}).values()]
            elif isinstance(val, list):
                labels += [str(v) for v in val]
    return labels


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent.parent
    bad = []
    jobs_checked = 0
    for path in sorted((root / ".github" / "workflows").glob("*.yml")):
        text = path.read_text(encoding="utf-8")
        try:
            doc = yaml.safe_load(text) or {}
        except yaml.YAMLError as exc:  # a malformed workflow is somebody else's gate
            print(f"{path.relative_to(root)}: could not parse: {exc}")
            return 1
        lines = text.splitlines()
        for name, job in (doc.get("jobs") or {}).items():
            if not isinstance(job, dict):
                continue
            if not any(MACOS.search(l) for l in runners_for(job)):
                continue
            jobs_checked += 1
            for step in job.get("steps") or []:
                script = (step or {}).get("run")
                if not isinstance(script, str):
                    continue
                for raw in script.splitlines():
                    if raw.strip().startswith("#"):
                        continue
                    for pat, what in BASH4:
                        if pat.search(raw):
                            # Report the line as it appears in the file, so the
                            # message is clickable rather than merely true.
                            # Compared stripped: YAML removes the block scalar's
                            # own indentation, so the string handed back here
                            # never matches the file's line verbatim — which is
                            # how the first version of this reported `:0`.
                            n = next((i for i, l in enumerate(lines, 1)
                                      if l.strip() == raw.strip()), 0)
                            bad.append((path.relative_to(root), n, name, what))
    if bad:
        print("workflow steps that can run on macOS use bash 4 features:")
        for rel, n, job, what in bad:
            print(f"  {rel}:{n}: job `{job}` uses {what}; macOS ships bash 3.2")
        print()
        print("On bash 3.2 these are errors rather than no-ops, and under `set -e`")
        print("they take the whole step with them — on the one platform whose")
        print("artefact then goes missing while every other platform succeeds.")
        return 1
    print(f"workflow bashisms: {jobs_checked} macOS-capable job(s), no bash 4 features")
    return 0


if __name__ == "__main__":
    sys.exit(main())
