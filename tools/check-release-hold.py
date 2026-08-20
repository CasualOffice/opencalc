#!/usr/bin/env python3
"""A release workflow cannot publish by accident.

The owner's instruction was that images are built but **not published** until
they say so, and then only just before the desktop shell. An instruction like
that is normally kept by somebody remembering it, which lasts until the person
who remembers is not in the room.

So it is checked instead. For every `release-*.yml`:

  1. it is triggered by tags or by hand — **never** by a push to a branch, so
     merging cannot publish;
  2. its tag patterns are component-scoped, because two release workflows
     watching one namespace means tagging either fires both;
  3. any `workflow_dispatch` has a `dry_run` input defaulting to **true**, so
     the manual path is safe unless a human deliberately says otherwise.

What this does not do is stop a release. Pushing `server-v0.1.0` publishes, and
that is the point: the hold is on *accidents*, not on intent.
"""

import pathlib
import sys

import yaml

RELEASES = sorted(pathlib.Path(".github/workflows").glob("release-*.yml"))


def main():
    if not RELEASES:
        print("no release workflows found", file=sys.stderr)
        return 1

    problems = []
    for path in RELEASES:
        spec = yaml.safe_load(path.read_text())
        # PyYAML reads the `on:` key as the boolean True.
        triggers = spec.get("on", spec.get(True)) or {}

        push = triggers.get("push") or {}
        if "branches" in push:
            problems.append(
                f"{path.name}: publishes on a push to {push['branches']} — "
                f"merging would release"
            )
        tags = push.get("tags") or []
        if not tags and "workflow_dispatch" not in triggers:
            problems.append(f"{path.name}: has no tag trigger and no manual trigger")
        for pattern in tags:
            # `v*` would be claimed by every component at once.
            if not pattern.split("-")[0].split("_")[0].isalpha() or pattern.startswith("v"):
                problems.append(
                    f"{path.name}: tag pattern {pattern!r} is not component-scoped; "
                    f"two workflows watching it means one tag fires both"
                )

        dispatch = triggers.get("workflow_dispatch")
        if isinstance(dispatch, dict):
            dry = (dispatch.get("inputs") or {}).get("dry_run")
            if dry is None:
                problems.append(
                    f"{path.name}: can be run by hand with no dry_run input, so the "
                    f"safe path is the one nobody chose"
                )
            elif dry.get("default") is not True:
                problems.append(
                    f"{path.name}: dry_run defaults to {dry.get('default')!r}; a "
                    f"manual run must not publish unless somebody says so"
                )

    # Every image a release publishes has a registry page in the repository.
    #
    # The alternative is pasting one into a web form on release day, where it is
    # never reviewed and goes stale the moment anything changes. A public image
    # with no description is also the one thing a prospective user sees first.
    import re

    for path in RELEASES:
        spec_text = path.read_text()
        for image in re.findall(r"- image: ([a-z0-9-]+)", spec_text):
            page = pathlib.Path(f"docs/registry/{image}.md")
            if not page.exists():
                problems.append(
                    f"{path.name}: publishes the image {image!r} and {page} does not "
                    f"exist, so it would land with no description"
                )

    if problems:
        print("release workflows that could publish by accident:", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        return 1

    names = ", ".join(p.stem for p in RELEASES)
    print(f"release hold: {len(RELEASES)} workflow(s) ({names}) publish only on a scoped tag")
    return 0


if __name__ == "__main__":
    sys.exit(main())
