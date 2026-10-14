// Mirrors ONLYOFFICE's published macro sample "Unmerge cell range":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/unmerge-cell-range/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/unmerge-cell-range.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: C5:D10 is no longer merged.

(function () {
  Api.GetActiveSheet().GetRange("C5:D10").UnMerge();
})();
