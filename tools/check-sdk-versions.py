#!/usr/bin/env python3
"""The SDK packages must agree on one version, and on each other's.

`REL-05`. Three packages are published together from one tag, and
`release-sdk.yml` checks that tag against **one** of them — `sheet`. Nothing
checked the other two, and nothing checked the version `@opencalc/react` pins
for `@opencalc/sheet`.

That pin is the dangerous one. Bumping the three `version` fields and leaving
the dependency at the old number publishes `react@0.0.1` depending on
`sheet@0.0.0`: npm resolves it, so nothing fails, and a consumer installs two
copies of the same package at different versions. It was found on the way to
`0.0.1`, one edit before it would have happened.

The same shape as `REL-04` one component over — a fact written down four times,
with a check on one of them.
"""

from __future__ import annotations

import json
import pathlib
import sys

PACKAGES = ("engine", "react", "sheet")


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent.parent
    seen: dict[str, str] = {}
    manifests: dict[str, dict] = {}
    for name in PACKAGES:
        path = root / "sdk" / "packages" / name / "package.json"
        try:
            data = json.loads(path.read_text())
        except (OSError, ValueError) as why:
            print(f"::error::could not read {path}: {why}")
            return 1
        manifests[name] = data
        seen[data.get("name", name)] = data.get("version", "")

    versions = set(seen.values())
    if len(versions) != 1:
        print("the SDK packages do not agree on a version:")
        for pkg, version in sorted(seen.items()):
            print(f"  {pkg}: {version}")
        print()
        print("They are published together from one tag, and `release-sdk.yml`")
        print("compares that tag with `sheet` alone — so a disagreement here")
        print("publishes whatever the other two happen to say.")
        return 1
    version = versions.pop()

    # The pin between them, which is the one that fails silently.
    bad = []
    for name, data in manifests.items():
        for field in ("dependencies", "peerDependencies"):
            for dep, want in (data.get(field) or {}).items():
                if dep.startswith("@opencalc/") and want != version:
                    bad.append(f"{data['name']} {field}.{dep} = {want}, not {version}")
    if bad:
        print("an SDK package pins a sibling at a different version:")
        for line in bad:
            print(f"  {line}")
        print()
        print("npm resolves this rather than refusing it, so nothing fails and a")
        print("consumer installs two copies of one package at two versions.")
        return 1

    print(f"sdk versions: {len(PACKAGES)} packages agree on {version}, sibling pins match")
    return 0


if __name__ == "__main__":
    sys.exit(main())
