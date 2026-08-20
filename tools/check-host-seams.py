#!/usr/bin/env python3
"""No engine crate names a platform.

[19](../docs/19-WORKSPACE-SCAFFOLD-DESIGN.md) boundary invariant 7 promises the
engine never forks on where it is running: the browser build and the native
build compile the *same* code, and anything platform-specific enters as a value
or a predicate the host supplies. `ADR-019` proposes that wording in place of
the capability trait that was promised and never written.

It has been kept by care. This keeps it by CI, because the failure mode is not
somebody deciding to fork — it is one `#[cfg(target_os = "…")]` added to make a
build pass, which is invisible in review and silently means the two hosts are no
longer running the same engine.

What is checked: no `cfg(target_*)` in the engine crates. What is deliberately
allowed:

  * `cfg(feature = "…")`. A feature is chosen by whoever assembles the build; a
    `cfg(target_os)` is chosen by the engine behind everybody's back. ADR-018
    gates text shaping this way on purpose.
  * `cfg(test)`, and the server and tool crates, which are native by definition
    and never compiled to WebAssembly.
"""

import pathlib
import re
import subprocess
import sys

# The crates the wasm gate compiles — anything not under server/ or tools/.
ENGINE = pathlib.Path("crates")
TARGET_CFG = re.compile(r"cfg\s*\(\s*[^)]*target_(?:arch|os|family|env|vendor|pointer_width)")


def main():
    if not ENGINE.is_dir():
        print(f"{ENGINE} is missing", file=sys.stderr)
        return 1

    # `git ls-files` so a stale build artefact under target/ is never read.
    listed = subprocess.run(
        ["git", "ls-files", "crates/**/*.rs"],
        capture_output=True, text=True, check=False,
    ).stdout.split()
    files = [pathlib.Path(f) for f in listed] or sorted(ENGINE.rglob("*.rs"))

    findings = []
    for path in files:
        try:
            text = path.read_text()
        except OSError:
            continue
        for number, line in enumerate(text.splitlines(), 1):
            if TARGET_CFG.search(line):
                findings.append(f"{path}:{number}: {line.strip()}")

    if findings:
        print("engine crates that fork on the platform:", file=sys.stderr)
        for finding in findings:
            print(f"  {finding}", file=sys.stderr)
        print(
            "\nThe browser and native builds must compile the same engine. Take the\n"
            "platform-specific part out as a value or a predicate the host supplies —\n"
            "`Environment` and `Cancel` are the two that exist — or, if it is genuinely\n"
            "a dependency choice, put it behind a Cargo feature and say so in an ADR.",
            file=sys.stderr,
        )
        return 1

    print(f"host seams: {len(files)} engine sources, no platform forks")
    return 0


if __name__ == "__main__":
    sys.exit(main())
