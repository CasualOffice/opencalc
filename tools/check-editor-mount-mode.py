#!/usr/bin/env python3
"""An editor mounted inside another page names the mode it wants.

`UX-EMBED-02` changed what an editor does when nobody says anything: a frame or
a shadow root now resolves to `embedded` instead of `standalone`, so a host's
document is not handed out on the strength of the host having said nothing.

That is the right default and it moved **underneath three shipped callers**.
The costly one was WOPI: `wopi` and `embedded` differ on exactly one axis,
`chrome`, and the `wopi` preset keeps `"web"` on purpose — the editor *is* the
frame and draws its own chrome, the way Office Online does. An unmarked WOPI
frame silently lost its header. Our own landing page lost the branding strip
and the download menu it exists to show off.

**None of that would have failed a test**, because each caller was still
perfectly valid: it asked for nothing and got a working editor, just not the
one it meant. The defect only exists relative to intent, and intent was not
written down anywhere a machine could read.

So: a mount **inside another page** must name its mode. A plain link that
navigates to the editor need not — that is the editor being the page, which is
what `standalone` means, and demanding a query string there would be noise.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(".")

# Where a caller can mount the editor. **Tests are not excluded**, and that was
# not the first plan: exempting them looked tidy until the gate flagged three
# WOPI fixtures carrying a modeless URL. They were not asserting the default —
# they were describing a deployment that can no longer be correct, which is a
# fixture testing something that cannot happen. Fixed rather than exempted.
SEARCH = ["webapp", "server", "desktop"]
SKIP_PARTS = {"node_modules", "pkg", "target", "dist", "test-results"}

# A mount *inside another page*: an iframe, or a frame's `src` assigned in
# script, or a configured editor URL a host will frame.
# **Any** URL naming `editor.html`, classified by what surrounds it — not by a
# per-mount pattern. The first version of this gate matched `editor_url` and a
# quoted string *on one line*, which caught three single-line test fixtures and
# missed the real default:
#
#     editor_url: std::env::var("OPENCALC_EDITOR_URL")
#         .unwrap_or_else(|_| "/editor/editor.html".to_owned()),
#
# Two lines, so the regression this gate exists for walked straight past it and
# the gate printed green. That is the same fault it is written to catch, one
# level up: a check answering a narrower question than the one it prints.
URL = re.compile(r"""["'`]([^"'`\s]*editor\.html[^"'`\s]*)["'`]""")

# A plain link *navigates* to the editor — that is the editor being the page,
# which is what `standalone` means. Demanding a query string there is noise.
NAVIGATION = re.compile(r"<a\b[^>]*\bhref\s*=", re.I)

# What makes an occurrence a *mount* rather than a mention. Matched over a small
# window ending at the URL's line, not the line alone — that is the whole repair
# over the first version. Without the window the real default is invisible;
# with the line-only rule replaced by "any URL at all" the gate flagged 18
# places, almost all of them a bare filename in an asset table, and a gate that
# cries wolf is one nobody reads.
MOUNT_CONTEXT = re.compile(r"editor_url|\.src\s*=|<iframe\b", re.I)
WINDOW = 3
HAS_MODE = re.compile(r"[?&]mode=")


def files():
    for base in SEARCH:
        root = ROOT / base
        if not root.is_dir():
            continue
        for path in root.rglob("*"):
            if not path.is_file():
                continue
            if SKIP_PARTS & set(path.parts):
                continue
            if path.suffix not in {".html", ".js", ".mjs", ".rs", ".ts"}:
                continue
            yield path


def main() -> int:
    problems = []
    checked = 0
    for path in files():
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        if "editor.html" not in text:
            continue
        lines = text.splitlines()
        for number, line in enumerate(lines, 1):
            if NAVIGATION.search(line):
                continue
            context = "\n".join(lines[max(0, number - WINDOW) : number])
            if not MOUNT_CONTEXT.search(context):
                continue
            for found in URL.finditer(line):
                url = found.group(1)
                checked += 1
                if HAS_MODE.search(url):
                    continue
                problems.append(
                    f"{path}:{number}: mounts the editor inside another page "
                    f"without naming a mode — `{url}`"
                )

    if problems:
        print("editor mounts that inherit whatever the default happens to be:", file=sys.stderr)
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        print(
            "\nAdd `?mode=` and say which one. A mount that asks for nothing is "
            "valid, which is what makes this quiet: it keeps working when the "
            "default moves, just not as the caller meant. `wopi` and `embedded` "
            "differ only in `chrome`, so the WOPI frame lost its header and "
            "nothing failed.",
            file=sys.stderr,
        )
        return 1

    print(f"editor mounts: {checked} in-page mount(s), every one names its mode")
    return 0


if __name__ == "__main__":
    sys.exit(main())
