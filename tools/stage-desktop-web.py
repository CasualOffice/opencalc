#!/usr/bin/env python3
"""Stage the editor's web assets for the desktop bundle, without the CJK fonts.

`desktop/tauri.conf.json` used to embed `webapp/` whole. Since `XL-FEAT-140`
that folder holds 59 MB of Noto CJK slices (`webapp/fonts/cjk/`), and every
installer grew by the same ~58 MB: the 0.0.4 draft's `.dmg` was 69 MB against
0.0.3's 10 MB (`TAURI-021`).

The desktop does not need them. Every desktop OS ships East Asian fonts — Yu
Gothic, Microsoft YaHei and Malgun Gothic on Windows; Hiragino, PingFang and
Apple SD Gothic on macOS — and every font stack the editor builds ends in a
generic family, so the webview draws CJK with them. The loader already treats a
missing manifest as "draw with the viewer's own fonts, and let an export name
what it could not draw" (`webapp/editor.paint.js`, `loadCjkManifest`), and in
the desktop it then fetches the slices from the public site for PDF embedding
when it is online.

Run by `beforeBuildCommand`, so the staging directory is produced by the build
itself in every checkout (`tools/check-desktop-build.py` requires that), and
git-ignored. Paths are resolved from this file, so the command's working
directory does not matter.
"""

import pathlib
import shutil
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "webapp"
DEST = ROOT / "desktop" / "web-dist"

# Relative to `webapp/`. Directories are skipped whole.
EXCLUDE = {
    "fonts/cjk",
    "__pycache__",
}


def skipped(rel: pathlib.PurePosixPath) -> bool:
    text = rel.as_posix()
    return any(text == e or text.startswith(e + "/") for e in EXCLUDE) or "__pycache__" in rel.parts


def main() -> int:
    if not (SRC / "pkg").is_dir():
        print(
            "webapp/pkg/ is missing: build the engine first (python3 tools/build-wasm.py); "
            "a bundle without it opens a blank window",
            file=sys.stderr,
        )
        return 1
    if DEST.exists():
        shutil.rmtree(DEST)
    files = size = 0
    for path in sorted(SRC.rglob("*")):
        rel = pathlib.PurePosixPath(path.relative_to(SRC).as_posix())
        if path.is_dir() or skipped(rel):
            continue
        target = DEST / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)
        files += 1
        size += path.stat().st_size
    print(f"staged {files} files, {size / 1e6:.1f} MB, into {DEST.relative_to(ROOT)} (without {', '.join(sorted(EXCLUDE))})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
