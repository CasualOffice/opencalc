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

No font is committed to this repository. A CJK face alone is tens of megabytes,
and which one to carry is a regional decision that belongs to a deployment
rather than to this project.
