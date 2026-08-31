#!/usr/bin/env python3
"""CI builds the desktop application, on every platform it is shipped for.

For the whole of `TAURI-001` nothing did. `desktop/` is its own cargo workspace
(`ADR-023`, so Tauri's dependency tree does not ride along in every
`--workspace` build), which is exactly what makes it invisible: `format`,
`lint`, `test`, `docs` and `platform` all pass over it without compiling a line
of it, so the application could stop building on main and every job stayed
green. `CI-014` is the same shape, one workspace earlier.

Four things are asserted, and each is a way the job has been or could be lost:

  1. **The job exists, on macOS, Windows and Linux.** Deleting it is a
     three-line diff in an 800-line workflow, and nothing else in the
     repository would notice. Dropping *one leg* of the matrix is smaller
     still, and the desktop shell is shipped for all three.

  2. **It bundles, rather than merely compiling.** `cargo build` succeeding is
     precisely the signal that is already known to be worthless here:
     `desktop/build.rs` exists because a green build produced an installer
     whose window opened blank, `webapp/pkg/` having been absent when
     `generate_context!` embedded the assets. So the job must run the engine
     build, produce a real bundle, and then *look for the bundle on disk* —
     `tauri build` has exited 0 having emitted nothing when a bundler was
     unavailable, and an exit code is not an artefact.

  3. **The configuration still permits a bundle.** `bundle.active: false` in
     `tauri.conf.json` turns the whole of (2) into a job that builds a binary
     and asserts nothing, with no error anywhere. The same for `frontendDist`
     pointing somewhere that does not exist: that is a compile error in CI and
     a working tree locally, which is the worst order to find it in.

  4. **A Windows icon exists**, because on Windows its absence is not a missing
     picture but a failed compile — `tauri-build` refuses to generate the
     Windows Resource file without `icons/icon.ico`. This one was not
     hypothetical: `desktop/` shipped with `icons/icon.png` alone, so the
     Windows build could never have succeeded, and nothing noticed for the
     same reason as everything else here — nothing built it.

What is deliberately NOT asserted: that the bundle is signed, or which format
each platform produces. No signing identity exists for this project, and the
formats are a cost trade a comment in the workflow argues for — pinning them
here would make that argument unchangeable rather than reviewed.
"""

import json
import pathlib
import re
import sys

WORKFLOWS = pathlib.Path(".github/workflows")
CONFIG = pathlib.Path("desktop/tauri.conf.json")
MANIFEST = pathlib.Path("desktop/Cargo.toml")

# The job name is the contract (docs/15 publishes the job list as one), so it is
# named here rather than discovered by searching for whatever happens to run
# Tauri. A rename is then a deliberate change to both files.
JOB = "desktop"

# Where `tauri build` leaves what it made, relative to `desktop/`.
BUNDLE_DIR = "target/release/bundle"

# Every platform the shell is shipped for, and the runner label that means it.
# `-latest` is not required: a job pinned to `macos-15` is still macOS.
PLATFORMS = {
    "macOS": re.compile(r"macos-"),
    "Windows": re.compile(r"windows-"),
    "Linux": re.compile(r"ubuntu-"),
}


def jobs(text):
    """Split a workflow into (name, body) at two-space indented keys.

    The same reader `check-ci-retry.py` uses, and for the same reason: the
    gates run before anything is installed, so parsing has to work with the
    standard library alone.
    """
    parts = text.split("\njobs:\n", 1)
    if len(parts) < 2:
        return
    body = parts[1]
    starts = [(m.group(1), m.start()) for m in re.finditer(r"^  ([a-z][a-z0-9-]*):$", body, re.M)]
    for i, (name, at) in enumerate(starts):
        end = starts[i + 1][1] if i + 1 < len(starts) else len(body)
        yield name, body[at:end]


def find_job(name):
    """The (path, body) of a job by name, across every workflow."""
    for path in sorted(WORKFLOWS.glob("*.yml")):
        for job, body in jobs(path.read_text(encoding="utf-8")):
            if job == name:
                return path, body
    return None, None


def commands(body):
    """The job with its comments removed.

    Not a nicety. The first version of this gate searched the raw job body for
    `tauri build`, and replacing the real command with `cargo build --release`
    **passed** — because the comment above the step explains that `tauri build`
    runs `cargo build --release`, and the comment was the match. A gate that a
    comment can satisfy is a gate that reads the argument for the code instead
    of the code, which is the sinner it was written to catch.
    """
    return "\n".join(line for line in body.splitlines() if not line.lstrip().startswith("#"))


def invoked(pattern, body):
    """The lines of `body` on which `pattern` is run as a command.

    A command starts a line — after the indent, and after an inline `run:`.
    Anything further along is an argument, a string, or (the case that fooled
    this gate) the text of an `echo` explaining what the step would have run.
    """
    command = re.compile(r"^\s*(?:- )?(?:run: *)?" + pattern)
    return [line for line in body.splitlines() if command.match(line)]


def steps(body):
    """The job's steps, split at the `- ` that begins each one.

    Six-space indent is this workflow's shape throughout. A step, rather than
    the whole job, is the unit that matters for "something *fails* when the
    bundle is missing": `exit 1` anywhere in an 80-line job says nothing about
    whether it is reached by the check that found no bundle.
    """
    starts = [m.start() for m in re.finditer(r"^      - ", body, re.M)]
    for i, at in enumerate(starts):
        yield body[at : starts[i + 1] if i + 1 < len(starts) else len(body)]


def check_workflow(problems):
    path, body = find_job(JOB)
    if body is None:
        problems.append(
            f"no workflow defines a `{JOB}` job, so nothing builds the desktop "
            f"application on any machine.\n"
            f"    Its own cargo workspace (ADR-023) means no other job compiles it."
        )
        return None

    body = commands(body)
    missing = [name for name, pattern in PLATFORMS.items() if not pattern.search(body)]
    if missing:
        problems.append(
            f"{path.name}: the `{JOB}` job does not build on {', '.join(missing)}.\n"
            f"    The shell is shipped for macOS, Windows and Linux; a platform "
            f"with no leg is a platform nobody builds."
        )

    # A bundle, not a binary. `tauri build` is the command that produces one;
    # `cargo build` is the command that produced the blank window.
    #
    # **Invoked, not merely mentioned** — the second thing this gate got wrong.
    # After comments were excluded, swapping the real command for `cargo build
    # --release` still passed, because the step's own error message says
    # "tauri build produced no bundle". So the match has to be a line that
    # *starts* a command; an `echo` naming it is the job talking about itself.
    if not invoked(r"(cargo\s+)?tauri build\b", body):
        problems.append(
            f"{path.name}: the `{JOB}` job never runs `tauri build`.\n"
            f"    `cargo build` succeeding is exactly the signal desktop/build.rs "
            f"exists because it was worthless: it produced an installer that "
            f"opened a blank window. The gate is that a bundle is produced."
        )

    # The engine the webview loads is generated and git-ignored, so a bundle
    # built without this step is a bundle with no engine in it. The out-dir has
    # to be on the invocation, not merely somewhere in the job: `wasm-pack
    # build` writing to its default `pkg/` inside the crate leaves
    # `webapp/pkg/` as empty as it found it.
    if not any("webapp/pkg" in line for line in invoked(r"wasm-pack build\b", body)):
        problems.append(
            f"{path.name}: the `{JOB}` job does not build `webapp/pkg/` with "
            f"wasm-pack before bundling.\n"
            f"    That directory is not committed. Bundling without it embeds an "
            f"asset store with no engine in it — a blank window, from a green build."
        )

    # And the artefact is looked for, not inferred: some step has to *fail*
    # when the bundle is not on disk. Both halves are needed — the job must
    # name where the bundle lands, and a step must exit non-zero over it —
    # because naming the path alone is satisfied by the matrix that names it
    # for the upload, which happily uploads nothing.
    checks_disk = any(
        BUNDLE_DIR in step and re.search(r"^\s*exit 1\s*$", step, re.M) for step in steps(body)
    )
    if not checks_disk:
        problems.append(
            f"{path.name}: no step in the `{JOB}` job fails when no bundle appears "
            f"under `{BUNDLE_DIR}`.\n"
            f"    `tauri build` has exited 0 having emitted nothing when a bundler "
            f"was unavailable. An exit code is not an artefact."
        )
    return path


def check_config(problems):
    if not CONFIG.exists():
        problems.append(f"{CONFIG} is missing, so there is nothing to bundle")
        return
    try:
        config = json.loads(CONFIG.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        problems.append(f"{CONFIG} is not valid JSON: {exc}")
        return

    if config.get("bundle", {}).get("active") is not True:
        problems.append(
            f'{CONFIG}: `bundle.active` is not `true`, so `tauri build` produces a '
            f"binary and no bundle — and says nothing about it."
        )

    # **Windows needs a `.ico`, and it is a compile error rather than a missing
    # picture.** `tauri-build` generates a Windows Resource file on every
    # Windows build, and when the icon is absent it returns
    # "`icons/icon.ico` not found; required for generating a Windows Resource
    # file during tauri-build" — so `desktop/` did not compile on Windows at
    # all, and could not have, from the day Tauri was added. It went unnoticed
    # for the same reason the whole of TAURI-001 did: nothing built it.
    #
    # `icons/icon.ico` is `tauri-build`'s own default when no `.ico` is listed,
    # so that is the path checked when the list has none.
    icons = config.get("bundle", {}).get("icon", [])
    ico = next((i for i in icons if isinstance(i, str) and i.endswith(".ico")), "icons/icon.ico")
    if not (CONFIG.parent / ico).is_file():
        problems.append(
            f"{CONFIG}: no Windows icon at `desktop/{ico}`.\n"
            f"    tauri-build fails the *compile* on Windows without it — "
            f'"required for generating a Windows Resource file" — so the '
            f"Windows leg of the desktop job cannot get as far as bundling."
        )

    # `frontendDist` is embedded into the binary at compile time. A path that
    # does not exist fails the *compile*, in CI, minutes in; and a staging
    # directory that is git-ignored exists on the machine that made it and
    # nowhere else, which is the same failure wearing a disguise.
    dist = config.get("build", {}).get("frontendDist")
    if not isinstance(dist, str):
        problems.append(
            f"{CONFIG}: `build.frontendDist` is not a path. The webview's assets "
            f"are embedded from it at compile time; there is no runtime fallback."
        )
    elif not dist.startswith(("http://", "https://")):
        resolved = (CONFIG.parent / dist).resolve()
        # A generated staging directory is a legitimate design — it is how the
        # `webapp/` filter this configuration does not have would be built (a
        # `beforeBuildCommand` copying an allowlist into it). What is not
        # legitimate is a path that exists only on the machine that made it, so
        # the exemption is exactly "the build produces it".
        if not resolved.is_dir() and not config.get("build", {}).get("beforeBuildCommand"):
            problems.append(
                f"{CONFIG}: `build.frontendDist` is `{dist}`, which is not a "
                f"directory in a fresh checkout ({resolved}), and no "
                f"`beforeBuildCommand` produces one.\n"
                f"    Every build embeds it at compile time. A staging directory "
                f"has to be generated by the build itself, not by whoever ran a "
                f"command once."
            )


def main():
    if not WORKFLOWS.is_dir():
        print(f"{WORKFLOWS} is missing", file=sys.stderr)
        return 1

    problems = []
    check_workflow(problems)
    check_config(problems)

    if problems:
        print("the desktop application is not built by CI:", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        print(
            "\nTAURI-001: `desktop/` is its own workspace, so no other job compiles "
            "it.\nRestore the job rather than the gate — a CI job with nothing "
            "asserting it exists\nis how this one went missing in the first place.",
            file=sys.stderr,
        )
        return 1

    # **The version is stated twice, and nothing compared them** (`REL-04`).
    #
    # `desktop/tauri.conf.json` names the version the installer and the release
    # notes carry; `desktop/Cargo.toml` names the one the binary reports. They
    # are the same fact written down in two places, which is the shape this
    # repository keeps being bitten by — `UX-CHR-03` found a hand-written
    # `v0.0.0` in the About dialog that was right only by coincidence.
    #
    # A mismatch is invisible until somebody compares an installer's name with
    # what the application says about itself, which is exactly when a bug report
    # becomes impossible to place.
    try:
        conf_version = json.loads(CONFIG.read_text())["version"]
    except (OSError, ValueError, KeyError) as why:
        print(f"::error::could not read a version from {CONFIG}: {why}")
        return 1
    manifest_version = None
    for line in MANIFEST.read_text().splitlines():
        stripped = line.strip()
        if stripped.startswith("version") and "=" in stripped:
            manifest_version = stripped.split("=", 1)[1].strip().strip('"')
            break
    if manifest_version is None:
        print(f"::error::{MANIFEST} states no version")
        return 1
    if manifest_version != conf_version:
        print(
            f"the desktop version is stated twice and the two disagree:\n"
            f"  {CONFIG}: {conf_version}\n"
            f"  {MANIFEST}: {manifest_version}\n\n"
            f"The first names the installer and the release notes; the second is what\n"
            f"the binary reports about itself. Bump both, in the same commit."
        )
        return 1

    print(f"desktop build: the `{JOB}` job bundles on {', '.join(PLATFORMS)}, and both versions say {conf_version}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
