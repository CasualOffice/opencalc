// Mirrors ONLYOFFICE's published macro sample "Wrap characters with dashes":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/wrap-characters-with-dashes/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/wrap-characters-with-dashes.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: each selected non-empty cell's text becomes its characters separated and surrounded by "---".

(function () {
  Api.GetSelection().ForEach(function (cell) {
    var v = cell.GetValue();
    if (v === null || v === undefined || v === "") return;
    v = String(v);
    if (/^---(?:.---)+$/.test(v)) return;
    var out = "---";
    for (var i = 0; i < v.length; i++) out += v.charAt(i) + "---";
    cell.SetValue(out);
  });
})();
