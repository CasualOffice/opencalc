#!/bin/sh
# Regenerate the LibreOffice-produced fixture.
#
# A second *writer*, which is the point: every other fixture here was written by
# this project, so the fidelity tests compare this engine against itself. This
# one carries formulas and the answers LibreOffice computed for them, which is an
# oracle we do not control.
#
# The output is not byte-stable across LibreOffice versions, so regenerating
# changes the manifest checksum. That is expected — update it deliberately, and
# notice if the *values* moved rather than only the bytes.
set -eu
soffice="${SOFFICE:-soffice}"
out="fixtures/corpus/libreoffice-formulas.xlsx"
tmp="$(mktemp -d)"
cat > "$tmp/source.csv" <<'CSV'
Item,Qty,Price,Total,Note
Widget,3,4.5,=B2*C2,plain text
Gadget,12,0.99,=B3*C3,"comma, inside"
Gizmo,7,120,=B4*C4,café ünïcode
,,,=SUM(D2:D4),
CSV
"$soffice" --headless --convert-to xlsx --outdir "$tmp" "$tmp/source.csv" >/dev/null
mv "$tmp/source.xlsx" "$out"
rm -rf "$tmp"
echo "wrote $out"
