#!/usr/bin/env python3
"""Every module specifier the server rewrites gets the tree's stamp.

`serve.py` exists to make a module's URL move when the tree's bytes move, so a
browser can never pair a new page with a four-hour-old script. It does that by
rewriting relative specifiers — `from "./editor.core.js"` becomes
`from "./editor.core.js?v=<stamp>"`.

**It declined to rewrite any specifier that already carried a query**, and said
so on purpose: the quote had to follow `.js` immediately "so a specifier that
already carries a query is left alone, which makes the rewrite idempotent".
Idempotency was a real requirement and the reasoning was sound; the conclusion
was one step too strong. A hand-written `?v=4` is not a stamp this server
produced, and leaving it alone means leaving it **unstamped forever** —
`ribbon.model.js?v=4` and `ribbon.icons.js?v=2` were fetched under a URL that
had not moved since the day someone typed the number, which is precisely the
stale-module hazard the file's own docstring is about.

`gate.served-tree.spec.mjs` caught it in a browser, and that is the right place
for the end-to-end claim. It is the wrong place for the *cause*: it reports a
list of unstamped URLs and names no reason, so the next person reads it as "the
ribbon is special" rather than "a query makes a module invisible to stamping".

Hence two assertions here, at the level the defect actually lives at:

1. **A specifier already carrying a query is stamped, not skipped** — and the
   rewrite stays idempotent, which is the property the old rule was protecting.
2. **No import in the served tree hand-writes a version.** Assertion 1 makes a
   hand-written query harmless; this one keeps it from coming back, because a
   number typed by a person is a number that stops moving.

A template literal — ``import(`./collab.js?b=${BUILD}`)`` — is still skipped,
for the reason it always was: it interpolates the stamp itself.
"""

from __future__ import annotations

import importlib.util
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
WEBAPP = ROOT / "webapp"
STAMP = "0123456789abcdef"


def load_serve():
    spec = importlib.util.spec_from_file_location("_serve", WEBAPP / "serve.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


# A relative specifier that hand-writes its own version, e.g. `"./ribbon.js?v=4"`.
HAND_VERSIONED = re.compile(
    r'(?:\bfrom|\bimport)\s*\(?\s*(["\'])(\.{1,2}/[\w./-]+\.js)\?[^"\']*\1'
)


def main() -> int:
    serve = load_serve()
    stamp_specifiers = serve.stamp_specifiers
    bad: list[str] = []

    # 1. A query does not make a module invisible to stamping.
    src = 'import { a } from "./ribbon.model.js?v=4";'
    got = stamp_specifiers(src, STAMP)
    if f"?v={STAMP}" not in got:
        bad.append(
            "a specifier carrying a query was skipped rather than stamped:\n"
            f"    in  {src}\n    out {got.strip()}"
        )
    if "?v=4" in got:
        bad.append(f"the hand-written version survived the rewrite: {got.strip()}")

    # The property the old rule was protecting. Stamping twice must not stack
    # queries, or every reload would lengthen the URL.
    once = stamp_specifiers('import { a } from "./editor.core.js";', STAMP)
    twice = stamp_specifiers(once, STAMP)
    if once != twice:
        bad.append(f"the rewrite is not idempotent:\n    once  {once.strip()}\n    twice {twice.strip()}")

    # A template literal interpolates the stamp itself and must stay untouched.
    tpl = "const m = await import(`./collab.js?b=${BUILD}`);"
    if stamp_specifiers(tpl, STAMP) != tpl:
        bad.append("a template-literal specifier was rewritten; it carries the tag already")

    # A bare specifier still stamps — without this the whole gate could pass
    # against a function that does nothing.
    bare = stamp_specifiers('import { a } from "./editor.core.js";', STAMP)
    if f"./editor.core.js?v={STAMP}" not in bare:
        bad.append(f"a bare specifier was not stamped at all: {bare.strip()}")

    # 2. Nothing in the served tree hand-writes a version.
    for js in sorted(WEBAPP.glob("*.js")):
        for m in HAND_VERSIONED.finditer(js.read_text(encoding="utf-8")):
            line = js.read_text(encoding="utf-8")[: m.start()].count("\n") + 1
            bad.append(
                f"{js.relative_to(ROOT)}:{line} hand-writes a version on {m.group(2)} — "
                "the server stamps it from the tree; a typed number stops moving"
            )

    if bad:
        print("module stamping is not total:\n")
        for b in bad:
            print(f"  * {b}")
        print(
            "\nA module whose URL does not move when the tree moves is served stale "
            "for as long as the cache holds it."
        )
        return 1

    print("module stamping: queries stamped, rewrite idempotent, no hand-written versions")
    return 0


if __name__ == "__main__":
    sys.exit(main())
