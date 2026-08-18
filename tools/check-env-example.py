#!/usr/bin/env python3
"""`.env.example` must not send a browser at a port the stack does not publish.

The quick start is `cp .env.example .env && docker compose up`, so whatever is
in this file *is* the deployment for anybody trying OpenCalc for the first time.

It shipped `OPENCALC_COLLAB_WS=ws://127.0.0.1:8443/collab`. The collaboration
server is `expose`d to the compose network and **not published to the host**, so
the browser dialled a port nothing was listening on and co-editing — the one
thing the demo exists to demonstrate — never connected. The code had been fixed
for exactly this (PROD-12) and the docs updated; the example file was left
carrying the value the fix removed, and the example file is what people copy.

So: every `127.0.0.1:PORT` this file points a *browser* at must be a port some
compose file actually publishes.
"""

import re
import sys
from pathlib import Path

ENV = Path(".env.example")
COMPOSE = [Path("docker-compose.yml"), Path("docker-compose.cluster.yml")]


def published_ports() -> set[str]:
    """Host-side ports from every `"HOST:CONTAINER"` mapping, defaults included."""
    ports: set[str] = set()
    for f in COMPOSE:
        if not f.exists():
            continue
        for mapping in re.findall(r'^\s*-\s*"([^"]+)"', f.read_text(), re.M):
            if ":" not in mapping:
                # `- "8443"` is `expose`, not a publish: reachable inside the
                # compose network and nowhere else. That distinction is the
                # entire defect this checks for, so it must not be blurred.
                continue
            # Everything before the **last** colon is the host side. Splitting on
            # the first would cut `${OPENCALC_HOST_PORT:-8080}` in half, since
            # the shell's default syntax contains one too.
            host = mapping.rsplit(":", 1)[0]
            default = re.search(r":-(\d+)\}", host)
            if default:
                ports.add(default.group(1))
            elif host.isdigit():
                ports.add(host)
    return ports


def main() -> int:
    if not ENV.exists():
        print(f"::error::{ENV} is missing", file=sys.stderr)
        return 1

    ports = published_ports()
    if not ports:
        print("::error::no published ports found in any compose file", file=sys.stderr)
        return 1

    problems = []
    for line in ENV.read_text().splitlines():
        line = line.strip()
        if line.startswith("#") or "=" not in line:
            continue
        name, value = line.split("=", 1)
        for port in re.findall(r"127\.0\.0\.1:(\d+)", value):
            if port not in sorted(ports):
                problems.append(
                    f"{ENV}: {name} points a browser at 127.0.0.1:{port}, "
                    f"which no compose file publishes (published: {', '.join(sorted(ports))})"
                )

    for problem in problems:
        print(f"::error::{problem}", file=sys.stderr)
    if problems:
        return 1
    print(f"env example: every referenced port is published ({', '.join(sorted(ports))})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
