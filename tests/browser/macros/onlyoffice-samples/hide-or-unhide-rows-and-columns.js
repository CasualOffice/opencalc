// Mirrors ONLYOFFICE's published macro sample "Hide or unhide rows and columns":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/hide-or-unhide-rows-and-columns/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/hide-or-unhide-rows-and-columns.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: the listed rows (1-based) and columns (1-based within A:P) flip from visible to hidden.

(function () {
  var sheet = Api.GetActiveSheet();
  var area = sheet.GetRange("A:P");
  var rows = [5, 8];
  var cols = [2, 4];
  var hidden = sheet.GetRows(rows[0]).GetHidden();
  rows.forEach(function (r) { sheet.GetRows(r).SetHidden(!hidden); });
  cols.forEach(function (c) { area.GetCols(c).SetHidden(!hidden); });
})();
