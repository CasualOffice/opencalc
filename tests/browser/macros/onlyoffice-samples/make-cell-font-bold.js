// Mirrors ONLYOFFICE's published macro sample "Make cell font bold":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/make-cell-font-bold/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/make-cell-font-bold.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: A2 is bold.

(function () {
  Api.GetActiveSheet().GetRange("A2").SetBold(true);
})();
