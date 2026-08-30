#!/usr/bin/env python3
"""Serve the OpenCalc demo locally with correct MIME types and no caching.

Usage: python3 webapp/serve.py [port]   (default 8099)

Pass `0` for the port to let the operating system pick a free one; the line
this prints on startup names the port it actually got. That is how a test can
stand a second server up without picking a number that another agent on the
same machine has already taken.
"""

import hashlib
import http.server
import io
import json
import os
import re
import sys
import pathlib
import urllib.parse

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8099
ROOT = os.path.dirname(os.path.abspath(__file__))

# What counts as "the editor's code": everything the page can end up executing
# or rendering from. `.wasm` is in the list because the engine is fetched by URL
# like everything else (`pkg/casual_calc_wasm_bg.wasm?b=…`), so a rebuilt engine
# behind an unchanged glue file has to move the stamp too — that is a live
# instance of the same bug, since nothing under `pkg/` is an `editor*.js` and
# the old mtime rule could not see it. `.html` is in the list because `embed.js`
# *fetches* `editor.html` and injects its body, so the markup is code as far as
# an embedded editor is concerned.
CODE_SUFFIXES = (".js", ".css", ".html", ".wasm")
# Top level and `pkg/`. Not `fonts/`: a font is not executed and cannot make a
# stale module look fresh, and hashing binaries nothing imports only makes the
# stamp move for reasons that are not about code.
CODE_DIRS = ("", "pkg")

# Who is answering on this port, and out of which checkout.
#
# `serve.py` looks identical from the outside whichever tree it serves, and
# several agents run browser suites on one machine against a shared port range.
# That is exactly how a Playwright run attached to *another* checkout's server,
# loaded that tree's editor, and passed on code the author had never written
# (`CI-025`). A caller can now ask before it trusts the port.
IDENTITY = "/__opencalc__"

# name -> ((mtime_ns, size), sha256). Re-read only when a file's stat moves, so
# the multi-megabyte wasm binary is not hashed on every page load.
_digests: dict[str, tuple[tuple[int, int], str]] = {}

# A relative module specifier in a script: `from "./editor.core.js"`,
# `import "./embed.js"`, `import("./collab.js")`.
#
# Anchored on the `from`/`import` in front of it so it cannot touch a string
# that merely looks like a path — `editor.dialogs.js` really does contain
# `from "Heading 3"` — and requiring the quote immediately after `.js` so a
# specifier that already carries a query is left alone, which makes the rewrite
# idempotent. ``import(`./collab.js?b=${BUILD}`)`` is a template literal and is
# skipped for both reasons; it carries the tag already.
MODULE_SPECIFIER = re.compile(
    r'((?:\bfrom|\bimport)\s*\(?\s*)(["\'])(\.{1,2}/[\w./-]+\.js)\2'
)

# What a stamp looks like, and the only thing this server will echo back into a
# script body from a query string.
STAMP = re.compile(r"[0-9a-f]{16}")


def stamp_specifiers(text, tag):
    """Give every module a URL that moves when the tree moves.

    The editor is one entry point and eighteen modules that import each other
    by bare relative path. `editor.html` named `editor.js?v=…` and *nothing
    else was stamped at all*: `editor.core.js` and its topic modules were
    fetched under plain URLs, and the two things the editor loads dynamically —
    `collab.js` and the wasm glue — were fetched under `?b=${BUILD}`, where
    `BUILD` came from `editor.js`'s query.

    That last hop is where it broke, and not only in the packaged case its
    fallback was written for. `export * from "./editor.core.js"` evaluates the
    dependency *before* `editor.js`'s own body runs, so the global that body
    sets was still undefined when `BUILD` was computed and `BUILD` fell through
    to the literal `"dev"`. `collab.js?b=dev` was the URL for every build this
    server has ever served: editing `collab.js` changed nothing a browser could
    see.

    Stamping the specifiers themselves fixes both halves at once. Each module's
    own `import.meta.url` now carries the tag, so `BUILD` reads it from there
    with no dependence on evaluation order; and every module — not just the one
    the page names — has a URL that changes when the tree's bytes change.

    Every specifier in one graph gets the *same* tag, which is the part that
    must not be got wrong: `editor.core.js` is imported by `editor.js` and by
    all thirteen topic modules, so two tags would be two module instances —
    one keystroke committing two edits, the failure `MNT-005` was about.
    """
    return MODULE_SPECIFIER.sub(
        lambda m: f"{m.group(1)}{m.group(2)}{m.group(3)}?v={tag}{m.group(2)}", text
    )


def code_files():
    """Every file the editor can pull in, relative to `ROOT`, sorted."""
    names = []
    for folder in CODE_DIRS:
        base = os.path.join(ROOT, folder)
        try:
            entries = os.listdir(base)
        except OSError:
            continue
        for entry in entries:
            if not entry.endswith(CODE_SUFFIXES):
                continue
            if not os.path.isfile(os.path.join(base, entry)):
                continue
            names.append(f"{folder}/{entry}" if folder else entry)
    return sorted(names)


def digest(name):
    """The sha256 of a served file, cached against its `stat`."""
    path = os.path.join(ROOT, name)
    stat = os.stat(path)
    key = (stat.st_mtime_ns, stat.st_size)
    cached = _digests.get(name)
    if cached and cached[0] == key:
        return cached[1]
    digester = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digester.update(chunk)
    value = digester.hexdigest()
    _digests[name] = (key, value)
    return value


def manifest():
    """`{name: {sha256, bytes}}` for the whole served tree."""
    out = {}
    for name in code_files():
        try:
            out[name] = {
                "sha256": digest(name),
                "bytes": os.path.getsize(os.path.join(ROOT, name)),
            }
        except OSError:
            continue
    return out


def build_stamp(files=None):
    """One tag for the whole tree, derived from the bytes it will serve.

    The rule this replaces stamped each asset from *its own* mtime, and only
    `editor*.js` at that. Two things fall out of that and both have cost time:

    * `editor.core.js` names no other module directly — it imports them as
      `./collab.js?b=${BUILD}` and `./pkg/casual_calc_wasm.js?b=${BUILD}`, where
      `BUILD` is whatever `?v=` the page put on `editor.js`. Under the old rule
      editing `collab.js` moved no `editor*.js` mtime, so `BUILD` did not move,
      so the module URL did not move, and Chrome went on serving the module it
      had already resolved. `no-store` does not evict a module that is in the
      module map.
    * mtime is not content. A branch switch or a fresh clone rewrites every
      mtime without changing a byte, which invalidates everything for nothing;
      and a restored file can come back with a *newer* mtime and identical
      bytes. The tag has to answer "are these the same bytes", so it is made of
      the bytes.

    A digest over the whole tree over-invalidates — editing `index.html` moves
    the editor's tag as well. On a `no-store` development server that costs one
    extra fetch of files already on localhost, and the failure it rules out cost
    an afternoon.
    """
    files = manifest() if files is None else files
    digester = hashlib.sha256()
    for name in sorted(files):
        digester.update(f"{name} {files[name]['sha256']}\n".encode("utf-8"))
    return digester.hexdigest()[:16]


class Handler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {
        **http.server.SimpleHTTPRequestHandler.extensions_map,
        ".wasm": "application/wasm",
        ".js": "text/javascript",
        ".xlsx": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    }

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def send_head(self):
        # Rewrite the `?v=` on the entry pages' asset links to the tree's stamp.
        #
        # `no-store` is not enough on its own: Chrome reuses an ES module across
        # a same-URL navigation, so an edited editor.js kept running its old
        # code and a fix looked like it had failed. The number in the HTML is
        # also a manual step, and a stale one silently invalidates whatever you
        # just tested.
        path = self.path.split("?")[0]
        if path.rstrip("/") == IDENTITY:
            return self.send_identity()
        # `embed.html` is here because it is the *other* way into the editor:
        # it imports `./embed.js`, which reads its own `?v=` and reuses it for
        # `editor.html`, `editor.css` and `editor.js`. Unstamped, that query is
        # absent and `embed.js` falls back to the constant `"dev"` — so an
        # embedded editor pinned every one of its modules, `collab.js`
        # included, to a URL that never changes at all.
        if path.rstrip("/") in ("/editor.html", "/index.html", "/embed.html", ""):
            return self.serve_versioned(path or "/index.html")
        if path.endswith(".js"):
            # A bare name is shimmed only for a real module *import*. A
            # `fetch()` of the same URL is somebody reading the source — most
            # often to check that the bytes on this port are this checkout's,
            # which is the whole point of `CI-025` — and handing that a
            # two-line re-export tells it the file is empty. Chrome, Firefox
            # and Safari all send `Sec-Fetch-Dest: script` for a module import
            # and `empty` for `fetch`; anything with no header at all is a
            # tool, and a tool wants the source.
            #
            # Found by the browser suite: `CI-025`'s own fix broke the origin
            # check `CI-025` inspired — "served editor.dialogs.js has no size
            # dialog", because the probe was reading the shim.
            wants_module = self.headers.get("Sec-Fetch-Dest") == "script"
            if wants_module and not urllib.parse.urlsplit(self.path).query:
                shim = self.serve_shim(path)
                if shim is not None:
                    return shim
            served = self.serve_module(path)
            if served is not None:
                return served
        return super().send_head()

    def serve_shim(self, path):
        """An unstamped module URL, re-exporting the stamped one.

        Stamping the specifiers gives every module one URL — but only for code
        this server rewrote. A caller that reaches in from outside and asks for
        the bare name gets a *different* URL, and a different URL is a second
        module instance with its own copy of the module-scope state. The
        browser suite does exactly this (`await import("/editor.sheets.js")`),
        and the second copy is silent: `newDocument()` cleared the duplicate's
        document and the page went on showing the file it had open.

        So a bare name is not served as a module any more. It is served as a
        re-export of the stamped one, which is the same live bindings out of
        the one real instance. Only for a request with no query at all — a
        specifier the editor itself wrote carries `?v=` or `?b=` and must be
        served as itself, and `pkg/`'s glue is reached that way.
        """
        target = pathlib.Path(self.translate_path(path))
        try:
            source = target.read_bytes().decode("utf-8")
        except (OSError, UnicodeDecodeError):
            return None
        specifier = f"./{target.name}?v={build_stamp()}"
        lines = [f'export * from "{specifier}";']
        # `export *` does not carry a default through, and wasm-pack's glue
        # exports its `init` that way.
        if re.search(r"^export\s+default\b", source, re.MULTILINE):
            lines.append(f'export {{ default }} from "{specifier}";')
        raw = ("\n".join(lines) + "\n").encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/javascript")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        return io.BytesIO(raw)

    def request_tag(self):
        """The stamp this request already carries, or the tree's current one.

        A module graph is stamped from its *entry*. `editor.html` fixes one tag
        and everything under it inherits that value rather than re-reading the
        tree, so a file saved half way through a page load cannot leave the
        page holding two URLs for one module.

        Only a stamp this server could have produced is echoed back: the value
        is written into a JavaScript body, and taking arbitrary text from a
        query string would be writing arbitrary text into a script.
        """
        query = urllib.parse.urlsplit(self.path).query
        given = urllib.parse.parse_qs(query).get("v", [""])[0]
        return given if STAMP.fullmatch(given) else build_stamp()

    def serve_module(self, path):
        """A script, with its own imports stamped. `None` if it is not one."""
        try:
            body = pathlib.Path(self.translate_path(path)).read_bytes().decode("utf-8")
        except (OSError, UnicodeDecodeError):
            return None
        raw = stamp_specifiers(body, self.request_tag()).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/javascript")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        return io.BytesIO(raw)

    def send_identity(self):
        """Report which tree this server serves, and what is in it.

        The root is the load-bearing field — a port is not an identity, and
        "the server answered" is not evidence that it answered out of your
        checkout. The manifest is there so a caller can say *which* file
        differs rather than only that something does.
        """
        files = manifest()
        body = json.dumps(
            {
                "server": "opencalc-serve.py",
                "root": ROOT,
                "pid": os.getpid(),
                "stamp": build_stamp(files),
                "modules": files,
            },
            indent=1,
            sort_keys=True,
        ).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        return io.BytesIO(body)

    def serve_versioned(self, path):
        name = path.lstrip("/") or "index.html"
        try:
            body = pathlib.Path(ROOT, name).read_bytes().decode("utf-8")
        except OSError:
            return super().send_head()

        tag = self.request_tag()
        # An asset the page already stamps by hand: re-point it at the tree.
        body = re.sub(
            r"((?:\./)?[\w./-]+\.(?:js|css))\?v=[\w.]+",
            lambda m: f"{m.group(1)}?v={tag}",
            body,
        )
        # A module specifier carrying no query at all — `embed.html`'s
        # `import "./embed.js"`. Anchored on the quotes so it cannot touch a
        # path that already has a query, or prose that merely names a file.
        body = re.sub(
            r'(?<=")(\./[\w./-]+\.js)(?=")',
            lambda m: f"{m.group(1)}?v={tag}",
            body,
        )
        raw = body.encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        return io.BytesIO(raw)


if __name__ == "__main__":
    os.chdir(ROOT)
    # Threading, not the plain HTTPServer this used to be.
    #
    # `HTTPServer` handles one connection at a time, and a browser holds its
    # connections open with keep-alive. So the *second* editor tab could not
    # load its modules while the first still had a socket open: its dynamic
    # `import("./collab.js")` simply never resolved, with no error anywhere —
    # the tab sat there looking like a collaboration bug.
    #
    # Two editors at once is the entire point of the collaboration demo, and of
    # the browser gate that drives two pages through this same server, so
    # serving them one at a time was never going to be enough.
    server = http.server.ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    bound = server.server_address[1]
    # The root, every time, and not only because a caller might want it: two
    # checkouts of this repository serve the same page from the same script,
    # and the only thing that tells them apart is this line.
    print(f"OpenCalc demo → http://localhost:{bound}/", flush=True)
    print(f"  serving {ROOT} (stamp {build_stamp()})", flush=True)
    server.serve_forever()
