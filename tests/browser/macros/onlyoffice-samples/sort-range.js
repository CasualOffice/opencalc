// Mirrors ONLYOFFICE's published macro sample "Sort range":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/sort-range/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/sort-range.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: the used range's rows are sorted by the first column numerically, ties by the second alphabetically.

(function () {
  var range = Api.GetActiveSheet().GetUsedRange();
  if (!range) return;
  var rows = range.GetValue();
  if (!rows.length) return;
  rows.sort(function (a, b) {
    var x = parseFloat(a[0]), y = parseFloat(b[0]);
    if (!isNaN(x) && !isNaN(y) && x !== y) return x - y;
    return String(a[1] || "").toLowerCase().localeCompare(String(b[1] || "").toLowerCase());
  });
  range.SetValue(rows);
})();
