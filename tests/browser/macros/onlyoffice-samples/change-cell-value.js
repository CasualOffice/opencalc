// Mirrors ONLYOFFICE's published macro sample "Change cell value":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/change-cell-value/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/change-cell-value.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: A1 is selected and holds "Hello World".

(function () {
  var sheet = Api.GetActiveSheet();
  sheet.GetRange("A1").Select();
  sheet.GetRange("A1").SetValue("Hello World");
})();
