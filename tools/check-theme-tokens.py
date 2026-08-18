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
"""

import re
import sys
from pathlib import Path

EMBED = Path("webapp/embed.js")
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
