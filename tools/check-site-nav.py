#!/usr/bin/env python3
"""The marketing site's navigation: no duplicates, nothing broken.

A duplicate entry is the specific mistake this exists for. `deploy.html` was
linked into every page's nav by a script that guarded against a repeated *link*
and not a repeated *label* — and `index.html` already carried a "Self-host"
anchor to its own section, so the homepage shipped two identical-looking nav
items pointing at different places.

Nothing catches that. It is not a broken link, not invalid HTML, and not visible
to any test that renders the page successfully; it just looks careless to
everyone who sees it.

Two rules, both checked per page:

* No two entries in one nav share a visible label. Same label, different target,
  is the failure above; same label, same target is a stray copy.
* **No two entries lead to the same place.** Two links to one page, however
  they are labelled, are a stray copy.

What this deliberately does **not** check: two entries about the same *subject*
at different destinations. The homepage once carried a `#selfhost` section
anchor beside a link to the self-hosting page, labelled differently — two routes
to one topic, and a reader had to guess which was real. That is the mistake this
gate was extended for, and it turned out not to be lintable: every mechanical
rule tried for it also flagged ordinary cross-linking, like the "Embed" section
pointing readers at the SDK guide. A check that cries wolf gets switched off, so
that one stays a review question and is written down here rather than pretended
away.
* Every relative target exists, and every `#anchor` has an element with that id
  on the page that links to it.
"""

import re
import sys
from pathlib import Path

SITE = Path("webapp")
NAV = re.compile(r'<nav class="nav-links">(.*?)</nav>', re.S)
LINK = re.compile(r'<a[^>]*href="([^"]+)"[^>]*>(.*?)</a>', re.S)


def text_of(markup: str) -> str:
    return re.sub(r"\s+", " ", re.sub(r"<[^>]+>", "", markup)).strip()


def main() -> int:
    problems = []
    checked = 0

    for page in sorted(SITE.glob("*.html")):
        source = page.read_text()
        nav = NAV.search(source)
        if not nav:
            continue  # The editor and the embed demo carry no marketing nav.
        checked += 1
        ids = set(re.findall(r'id="([^"]+)"', source))

        seen: dict[str, str] = {}
        destinations: dict[str, str] = {}
        for href, label_markup in LINK.findall(nav.group(1)):
            label = text_of(label_markup)
            if not label:
                continue
            if label in seen:
                problems.append(
                    f"{page}: two nav entries are both labelled {label!r} "
                    f"({seen[label]} and {href})"
                )
            seen[label] = href

            if href in destinations:
                problems.append(
                    f"{page}: {destinations[href]!r} and {label!r} both go to {href}"
                )
            destinations[href] = label

            if href.startswith("http"):
                continue
            if href.startswith("#"):
                if href[1:] not in ids:
                    problems.append(f"{page}: nav links to {href}, which is not on the page")
                continue
            target = href.split("#")[0].split("?")[0]
            if target and not (SITE / target).exists():
                problems.append(f"{page}: nav links to {target}, which does not exist")

    if not checked:
        problems.append("no pages with a marketing nav were found — is this reading the right directory?")

    for problem in problems:
        print(f"::error::{problem}", file=sys.stderr)
    if problems:
        return 1
    print(f"site nav: {checked} pages, no duplicate labels, no broken targets")
    return 0


if __name__ == "__main__":
    sys.exit(main())
