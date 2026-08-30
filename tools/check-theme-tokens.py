#!/usr/bin/env python3
"""Every theme token the SDK advertises must actually do something.

`THEME_TOKENS` is a published list: a host reads it to build a theme picker and
sets the ones it cares about. A name on that list that the stylesheet never
mentions is a promise the product does not keep — the host sets it, nothing
changes, and there is no error to explain why.

`dangerColor` was exactly that. It sat in the published list while
`--oc-danger-color` appeared nowhere in the CSS, and two different reds were
spelled out by hand instead.

Checked in one direction: every advertised token must be **defined or
referenced**. A `var(--oc-x, fallback)` counts, because that is a working
override even without a declared default. The stylesheet may hold tokens the
list does not advertise — those are internal.

**And the names themselves are checked against a manifest** (`DOC-034`). This
gate could say every advertised token was honoured and nothing could say they
were the names anybody meant to publish — which is how `docs/55`'s decision 3,
"rename the theme tokens to typed names now, cheap now and impossible after the
first release", got answered by the release happening. `sdk/theme-tokens.json`
is that answer written down. It is public API: a host sets these to theme an
embed, so renaming one breaks every consumer stylesheet that set it. Changing
the set is now a deliberate diff in a file whose whole purpose is to be read,
rather than a silent edit to a list inside a script.
"""

import json

import re
import sys
from pathlib import Path

EMBED = Path("webapp/embed.js")
# The published names, as a file somebody has to edit on purpose (`DOC-034`).
MANIFEST = Path("sdk/theme-tokens.json")
CSS = [Path("webapp/editor.css"), Path("webapp/style.css")]


def kebab(name: str) -> str:
    return "--oc-" + re.sub(r"(?<!^)(?=[A-Z])", "-", name).lower()


def main() -> int:
    source = EMBED.read_text()
    block = re.search(r"const TOKENS = \[(.*?)\]", source, re.S)
    if not block:
        print("::error::could not find the TOKENS list in webapp/embed.js", file=sys.stderr)
        return 1
    advertised = re.findall(r'"([A-Za-z]+)"', block.group(1))

    # The published set, compared as an ordered list: order is what a host sees
    # when it builds a picker from this, so a reshuffle is a visible change too.
    try:
        published = json.loads(MANIFEST.read_text())["tokens"]
    except (OSError, ValueError, KeyError) as why:
        print(f"::error::could not read {MANIFEST}: {why}", file=sys.stderr)
        return 1
    if advertised != published:
        added = [t for t in advertised if t not in published]
        gone = [t for t in published if t not in advertised]
        print(f"the advertised theme tokens are not the published ones ({MANIFEST}):")
        if added:
            print(f"  advertised and not published: {added}")
        if gone:
            print(f"  published and not advertised: {gone}")
        if not added and not gone:
            print("  same names, different order — a host builds its picker from this order")
        print()
        print("These names are public API: a host sets them to theme an embed, so")
        print("renaming one breaks every consumer stylesheet that set it. If the change")
        print("is intended, edit the manifest in the same commit and say so.")
        return 1
    if len(advertised) < 10:
        print(f"::error::only {len(advertised)} tokens parsed — the list shape changed", file=sys.stderr)
        return 1

    css = "".join(f.read_text() for f in CSS if f.exists())
    missing = [name for name in advertised if kebab(name) not in css]

    for name in missing:
        print(
            f"::error::the SDK advertises the theme token {name!r} "
            f"({kebab(name)}), which no stylesheet defines or uses — setting it does nothing",
            file=sys.stderr,
        )
    if missing:
        return 1
    print(f"theme tokens: all {len(advertised)} advertised tokens are honoured")
    return 0


if __name__ == "__main__":
    sys.exit(main())
