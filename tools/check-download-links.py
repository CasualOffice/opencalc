#!/usr/bin/env python3
"""The marketing site's download links point at files the release actually has.

`webapp/download.html` hard-codes five URLs of the form

    .../releases/download/desktop-v0.0.1/OpenCalc_0.0.1_aarch64.dmg

and nothing anywhere else in the repository knows that those strings exist.
The version appears twice in each one — once in the tag, once in the file name
that Tauri derives from `desktop/tauri.conf.json` — so a release is two edits
away from a page of 404s, and the page renders identically either way. A dead
download link is invisible to every other gate here: it is not a broken *doc*
reference (`check-doc-references.py` resolves repository paths, and these are
absolute URLs to a host that is not consulted), it is not a version
disagreement between manifests (`check-sdk-versions.py` compares the three npm
packages, which have nothing to do with the desktop bundle), and it is not a
build failure, because the build is what produces the *correct* names while the
page keeps pointing at the old ones.

The names are not guessed. Tauri composes them as
`{productName}_{version}_{arch}.{ext}`, and this release confirmed all four
against the real bundler output: `OpenCalc_0.0.1_aarch64.dmg`,
`OpenCalc_0.0.1_amd64.deb`, `OpenCalc_0.0.1_amd64.AppImage` and
`OpenCalc_0.0.1_x64-setup.exe`. So three things are asserted:

  1. **Every tag in the page is the version the desktop shell is at.** This is
     the half that breaks first, because bumping `tauri.conf.json` is the act
     that makes the page wrong and touches nothing near it.

  2. **Every asset name carries that same product name and version.** The tag
     can be right while the file name is stale; GitHub then serves a 404 from
     a release that exists, which is the more confusing of the two failures.

  3. **All four platforms are still linked.** Deleting a card from the page is
     a small diff, and losing one platform's download silently is exactly the
     shape of `TAURI-001`: shipped for three, checked for none.

What is deliberately NOT asserted: that the URLs resolve. That needs the
network and a published release, so it would fail on every branch cut before a
tag exists and turn a gate into a flake. The names are checked against their
generator instead, which is what actually drifts.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PAGE = ROOT / "webapp" / "download.html"
CONF = ROOT / "desktop" / "tauri.conf.json"

# arch fragment -> the platform it is the download for, so a missing card names
# the user who lost their download rather than a string that vanished.
PLATFORMS = {
    "aarch64.dmg": "macOS (Apple silicon)",
    "amd64.deb": "Linux (Debian/Ubuntu)",
    "amd64.AppImage": "Linux (AppImage)",
    "x64-setup.exe": "Windows",
}

# Both patterns are scanned over the whole file rather than over `href`
# attributes, because the page carries the version in four places no href
# parser sees: the `BASE` constant of the OS-detection script, and the three
# bare file names it concatenates onto it. Those are the links a visitor on the
# matching platform is actually offered, so missing them would leave the gate
# checking only the fallback cards.
TAG = re.compile(r"desktop-v([0-9][^/\"\s]*)")
ASSET = r"{product}_([0-9][^_\"\s]*)_([^\"\s<]+)"


def main() -> int:
    conf = json.loads(CONF.read_text())
    product, version = conf["productName"], conf["version"]
    expected_tag = f"desktop-v{version}"
    prefix = f"{product}_{version}_"

    text = PAGE.read_text()
    tags = TAG.findall(text)
    assets = re.findall(ASSET.format(product=re.escape(product)), text)
    if not tags or not assets:
        print(f"FAIL: {PAGE.relative_to(ROOT)} links to no release assets at all")
        return 1

    problems = []
    for tag in sorted(set(tags)):
        if tag != version:
            problems.append(
                f"  tag desktop-v{tag}: desktop/tauri.conf.json is at {version}, "
                f"so the release is {expected_tag}"
            )
    for ver, rest in sorted(set(assets)):
        if ver != version:
            problems.append(
                f"  {product}_{ver}_{rest}: Tauri names this bundle "
                f"{prefix}<arch>.<ext>, so this URL 404s even when the tag is right"
            )

    linked = {rest for _, rest in assets}
    for frag, platform in PLATFORMS.items():
        if not any(a.endswith(frag) or a == frag for a in linked):
            problems.append(f"  no download for {platform} (nothing ends in {frag})")

    if problems:
        print(f"FAIL: {PAGE.relative_to(ROOT)} would serve broken downloads:")
        print("\n".join(problems))
        return 1

    print(
        f"ok: {len(tags)} tag and {len(assets)} asset references on the "
        f"download page agree with {product} {version}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
