#!/bin/sh
# Regenerate the `.ods` corpus.
#
# **LibreOffice writes these, not this project.** That is the whole point: the
# ODS reader was tested against documents this repository produced, which proves
# the reader agrees with the writer and says nothing about whether either agrees
# with the application that actually makes `.ods` files. Four of the five
# defects found in that reader by hand were parser-shaped — a repeat count on
# the wrong variable, a self-closing cell that skipped the cursor, an unescaping
# step that never ran, a bound clamped in the wrong place — and every one of
# them is a mistake a real LibreOffice document walks straight into, because
# LibreOffice writes repeat runs and escaped text on nearly every row.
#
# Each source below is a CSV, so what is committed is a document LibreOffice
# authored from scratch: its own repeat attributes, its own escaping, its own
# element order. Formulas are evaluated on CSV import, so the cells carry both
# a formula and the answer LibreOffice computed for it.
#
# The output is not byte-stable across LibreOffice versions. Regenerating is a
# deliberate, reviewed change — and the thing to notice afterwards is whether
# the *values* moved, not whether the bytes did.
#
#     SOFFICE=/path/to/soffice tools/casual-calc-fidelity/corpus/ods/generate.sh
set -eu
soffice="${SOFFICE:-soffice}"
out="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# 1. Values, formulas and the text that has to survive escaping. The `&`, the
#    angle brackets, the quotes and the accented characters are all written by
#    LibreOffice as entities or as UTF-8 it expects back verbatim.
cat > "$tmp/values-and-formulas.csv" <<'CSV'
Item,Qty,Price,Total,Note
Widget,3,4.5,=B2*C2,"a & b < c > d"
Gadget,12,0.99,=B3*C3,"comma, inside"
Gizmo,7,120,=B4*C4,café ünïcode
Sum,,,=SUM(D2:D4),"apostrophe's ""quoted"""
Logic,,,=D5>100,plain
CSV

# 2. Gaps. LibreOffice compresses every run of empty cells into
#    `table:number-columns-repeated` and every run of empty rows into
#    `table:number-rows-repeated`, so a reader that mishandles either puts
#    everything after the gap in the wrong place — a corrupt document that
#    opens without complaint. The far-right value is the assertion.
cat > "$tmp/gaps-and-repeats.csv" <<'CSV'
left,,,,,,,,,far right
,,,,,,,,,
,,,,,,,,,
below,,,,,,,,,end
CSV

# 3. Text edges: leading and trailing spaces, a cell that is only spaces, one
#    that looks numeric, and one carrying a line break — which LibreOffice
#    writes as two `<text:p>` elements inside one cell.
printf '%s\n' \
  'label,value' \
  '"  padded  ",1' \
  '"   ",2' \
  '"0012",3' \
  '"first line' \
  'second line",4' \
  > "$tmp/text-edges.csv"

for source in values-and-formulas gaps-and-repeats text-edges; do
  "$soffice" --headless --convert-to ods --outdir "$tmp" "$tmp/$source.csv" >/dev/null
  mv "$tmp/$source.ods" "$out/$source.ods"
  echo "wrote $out/$source.ods"
done
