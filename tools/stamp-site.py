#!/usr/bin/env python3
"""Stamp an assembled static site the way `webapp/serve.py` stamps the dev tree.

    python3 tools/stamp-site.py site/

GitHub Pages serves files as they are, so the hand-written `?v=` numbers in
`editor.html` never moved between deploys: a browser holding `ribbon.js?v=19`
or a cached 404 for a file a deploy had just added kept using it for the
cache lifetime (four hours on Pages). The ribbon's icon sprite went missing
on calc.casualoffice.org that way after it had been fixed.

This applies `serve.py`'s own rules — imported, not copied, so the deploy and
the dev server cannot drift — with one tag for the whole site, hashed from the
code files it serves. Every module URL and every stamped asset link changes
exactly when any code byte changes.
"""

import hashlib
import importlib.util
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CODE_SUFFIXES = (".js", ".css", ".html", ".wasm", ".svg")


def load_serve():
    spec = importlib.util.spec_from_file_location("serve", ROOT / "webapp" / "serve.py")
    module = importlib.util.module_from_spec(spec)
    argv, sys.argv = sys.argv, [sys.argv[0]]   # serve.py reads a port from argv
    try:
        spec.loader.exec_module(module)
    finally:
        sys.argv = argv
    return module


def site_tag(site):
    digest = hashlib.sha256()
    for path in sorted(site.rglob("*")):
        if path.is_file() and path.suffix in CODE_SUFFIXES:
            digest.update(str(path.relative_to(site)).encode())
            digest.update(path.read_bytes())
    return digest.hexdigest()[:16]


def main():
    site = pathlib.Path(sys.argv[1]).resolve()
    serve = load_serve()
    tag = site_tag(site)
    assert serve.STAMP.fullmatch(tag), tag
    scripts = pages = 0
    for path in site.rglob("*.js"):
        text = path.read_text(encoding="utf-8")
        stamped = serve.stamp_specifiers(text, tag)
        if stamped != text:
            path.write_text(stamped, encoding="utf-8")
            scripts += 1
    for path in site.rglob("*.html"):
        text = path.read_text(encoding="utf-8")
        # The same two rewrites `serve.py`'s `send_head` applies to a page.
        text = re.sub(r"((?:\./)?[\w./-]+\.(?:js|css))\?v=[\w.]+", lambda m: f"{m.group(1)}?v={tag}", text)
        text = re.sub(r'(?<=")(\./[\w./-]+\.js)(?=")', lambda m: f"{m.group(1)}?v={tag}", text)
        path.write_text(text, encoding="utf-8")
        pages += 1
    print(f"stamped {scripts} scripts and {pages} pages with {tag}")


if __name__ == "__main__":
    main()
