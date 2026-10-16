# Fonts for the headless renderer

Drop `.ttf`, `.otf` or `.ttc` files here. The host serves them, the editor
registers them at boot, and the headless PNG renderer searches them **before**
its own bundled faces — in filename order, so the same document renders in the
same face on every boot.

Nothing here is required. Latin renders with no configuration at all; this
directory is for the scripts that do not, and
[docs/65-RUNNING-IT.md](../docs/65-RUNNING-IT.md) lists which face covers which.

Only the **headless** renderer cares. The editor draws every cell a person looks
at through the browser, using the browser's fonts, in every script — whether or
not anything is in here.

No font is committed to this directory. Japanese, Chinese and Korean are not
needed here: they ship as unicode-range slices in `webapp/fonts/cjk/`, which
the editor fetches on demand and hands to the exporters, and which a native
host loads with `casual_calc_render::cjk::SliceDirectory`
([docs/101](../docs/101-CJK-FONTS.md), `XL-FEAT-140`). This directory is for
the other scripts — Arabic, Devanagari, Thai — whose faces remain a
deployment's choice.
