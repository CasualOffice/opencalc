// Mirrors ONLYOFFICE's published macro sample "Merge cell range":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/merge-cell-range/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/merge-cell-range.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: A1:B3 merged across: A1:B1, A2:B2 and A3:B3 are each one cell.

(function () {
  Api.GetActiveSheet().GetRange("A1:B3").Merge(true);
})();
