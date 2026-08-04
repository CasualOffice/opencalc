#!/usr/bin/env python3
"""Serve the OpenCalc demo locally with correct MIME types and no caching.

Usage: python3 webapp/serve.py [port]   (default 8099)
"""

import http.server
import os
import sys

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


if __name__ == "__main__":
    os.chdir(ROOT)
    print(f"OpenCalc demo → http://localhost:{PORT}/")
    http.server.HTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
