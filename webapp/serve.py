#!/usr/bin/env python3
"""Serve the OpenCalc demo locally with correct MIME types and no caching.

Usage: python3 webapp/serve.py [port]   (default 8099)
"""

import http.server
import io
import os
import re
import sys
import pathlib

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8099
ROOT = os.path.dirname(os.path.abspath(__file__))


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
        # Rewrite the `?v=` on editor.html's asset links to each file's mtime.
        #
        # `no-store` is not enough on its own: Chrome reuses an ES module across
        # a same-URL navigation, so an edited editor.js kept running its old
        # code and a fix looked like it had failed. The number in the HTML is
        # also a manual step, and a stale one silently invalidates whatever you
        # just tested.
        if self.path.split("?")[0].rstrip("/") in ("/editor.html", "/index.html", ""):
            return self.serve_versioned(self.path.split("?")[0] or "/index.html")
        return super().send_head()

    def serve_versioned(self, path):
        name = path.lstrip("/") or "index.html"
        try:
            body = pathlib.Path(ROOT, name).read_bytes().decode("utf-8")
        except OSError:
            return super().send_head()

        def stamp(m):
            asset = m.group(1)
            try:
                return f'{asset}?v={int(os.path.getmtime(os.path.join(ROOT, asset)))}'
            except OSError:
                return m.group(0)

        body = re.sub(r'(?:\./)?([\w./-]+\.(?:js|css))\?v=\d+', stamp, body)
        raw = body.encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        return io.BytesIO(raw)


if __name__ == "__main__":
    os.chdir(ROOT)
    print(f"OpenCalc demo → http://localhost:{PORT}/")
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
    http.server.ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
