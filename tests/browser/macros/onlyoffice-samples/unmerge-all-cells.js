// Mirrors ONLYOFFICE's published macro sample "Unmerge all cells":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/unmerge-all-cells/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/unmerge-all-cells.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: no merged cells remain in the used range.

(function () {
  var used = Api.GetActiveSheet().GetUsedRange();
  if (used) used.UnMerge();
})();
