#!/usr/bin/env python3
"""No chrome rule references a custom property that nothing defines.

The ribbon and Compact chromes are drawn against a visual contract ported from
the sibling editor at `../opendoc` (its `docs/123`, measured first-hand from
ONLYOFFICE 9.4.1 and a live Google Docs). That stylesheet depends on 29 custom
properties, and when the port began **not one of them was defined here**.

An undefined custom property is the worst possible failure mode for a port,
which is why this is a gate and not a review note:

  * `var(--ribbon-band-h)` with nothing behind it does not error, does not warn,
    and does not show up in any console. The declaration is simply dropped and
    the element takes its initial value — so a band with no height collapses to
    its content and *looks plausible*, just wrong by 20px.
  * It is invisible to every assertion that measures something else. The suite
    can be entirely green while half the contract is inert.
  * And it is directional: a token that resolves in light mode can still be
    missing from the dark block, so a light-mode screenshot proves nothing.

So: every `var(--x)` reachable from a chrome selector must be defined, and the
two halves of the shim are checked for the properties that make them work —
the colour half must **alias** rather than restate (or dark mode and host
theming silently stop applying), and the geometry half must be **literal** (an
alias there would mean the measured number lives somewhere else).

`--x` used with a fallback — `var(--x, 12px)` — is exempt. That form is a
deliberate default and is how the published theme contract already works.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
WEBAPP = ROOT / "webapp"

# Stylesheets that draw chrome, and everything that may define a token for them.
CHROME_SHEETS = ["ribbon.css", "sheets.css"]
DEFINING_SHEETS = ["editor.css", "ribbon.tokens.css", "style.css"]

# A chrome selector: the ribbon, the Compact bar, or a chrome-scoped body class.
CHROME_SELECTOR = re.compile(r"\.(rb-|gs-|oc-chrome-|oc-ribbon|oc-sheets)")

# `var(--x)` with NO fallback. A fallback is a deliberate default, not a hole.
VAR_NO_FALLBACK = re.compile(r"var\(\s*(--[\w-]+)\s*\)")

# The two halves of the shim, and the property each must have.
MUST_ALIAS = [
    "--bg", "--bg-2", "--surface", "--ink", "--muted", "--faint",
    "--line", "--line-strong", "--accent-text", "--accent-soft", "--shadow-popover",
]
MUST_BE_LITERAL = [
    "--fs-body", "--fs-small", "--fs-caption", "--fs-ribbon-tab", "--fs-ribbon-caption",
    "--h-header", "--h-tab", "--ribbon-band-h", "--icon-control",
    "--radius", "--radius-sm", "--radius-popover",
]


def rule_blocks(text: str):
    return re.findall(r"([^{}]+)\{([^{}]*)\}", text)


def declared(text: str) -> dict[str, str]:
    """Custom property -> its declared value, last declaration winning."""
    out: dict[str, str] = {}
    for m in re.finditer(r"(--[\w-]+)\s*:\s*([^;}]+)", text):
        out[m.group(1)] = m.group(2).strip()
    return out


def main() -> int:
    bad: list[str] = []

    defined: dict[str, str] = {}
    for name in DEFINING_SHEETS:
        p = WEBAPP / name
        if p.exists():
            defined.update(declared(p.read_text(encoding="utf-8")))

    # 1. Every token a chrome rule reaches for is defined somewhere.
    used_anywhere = 0
    for name in CHROME_SHEETS:
        p = WEBAPP / name
        if not p.exists():
            continue
        text = p.read_text(encoding="utf-8")
        for selector, body in rule_blocks(text):
            if not CHROME_SELECTOR.search(selector):
                continue
            for tok in VAR_NO_FALLBACK.findall(body):
                used_anywhere += 1
                if tok not in defined:
                    line = text[: text.index(body)].count("\n") + 1
                    bad.append(
                        f"webapp/{name}:~{line} uses {tok}, which nothing defines — "
                        f"the declaration is dropped silently\n      in: {selector.strip()[:78]}"
                    )

    # Not a vacuous pass: the sweep has to have found something to check.
    if used_anywhere == 0:
        bad.append(
            "no chrome rule referenced any custom property at all — the selector "
            "pattern has stopped matching, so this gate is checking nothing"
        )

    tokens_file = WEBAPP / "ribbon.tokens.css"
    if not tokens_file.exists():
        bad.append("webapp/ribbon.tokens.css is missing; the chrome has no token spine")
    else:
        shim = declared(tokens_file.read_text(encoding="utf-8"))

        # 2. The colour half aliases. A literal here means dark mode and host
        #    theming quietly stop reaching the chrome.
        for tok in MUST_ALIAS:
            val = shim.get(tok)
            if val is None:
                bad.append(f"{tok} is not defined in ribbon.tokens.css")
            elif "var(--oc-" not in val:
                bad.append(
                    f"{tok} is `{val}` — it must alias an --oc-* token, not restate a value. "
                    "A literal is dark-blind and ignores a host's theme."
                )

        # 3. The geometry half is literal. An alias here means the measured
        #    number from docs/123 is no longer stated where it is documented.
        for tok in MUST_BE_LITERAL:
            val = shim.get(tok)
            if val is None:
                bad.append(f"{tok} is not defined in ribbon.tokens.css")
            elif "var(" in val:
                bad.append(
                    f"{tok} is `{val}` — a measured value must be literal here, "
                    "or the number no longer lives where its source is recorded."
                )

    # 4. The icon font is self-hosted. A CDN breaks offline use and the CSP.
    for name in DEFINING_SHEETS + CHROME_SHEETS:
        p = WEBAPP / name
        if not p.exists():
            continue
        for m in re.finditer(r"src\s*:\s*url\(\s*['\"]?([^'\")]+)", p.read_text(encoding="utf-8")):
            url = m.group(1)
            if url.startswith(("http://", "https://", "//")):
                bad.append(f"webapp/{name} fetches a font from {url} — it must be self-hosted")

    if bad:
        print("chrome tokens do not resolve:\n")
        for b in bad:
            print(f"  * {b}")
        print(
            "\nAn undefined custom property is dropped silently: no error, no warning, "
            "and an element that looks plausible at the wrong size."
        )
        return 1

    print(
        f"chrome tokens: {used_anywhere} references across {len(CHROME_SHEETS)} sheets all resolve; "
        f"{len(MUST_ALIAS)} aliased, {len(MUST_BE_LITERAL)} literal, font self-hosted"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
