// Mirrors ONLYOFFICE's published macro sample "Set column width":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/set-column-width/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/set-column-width.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: Column B (0-based index 1) is 25 characters wide.

(function () {
  Api.GetActiveSheet().SetColumnWidth(1, 25);
})();
