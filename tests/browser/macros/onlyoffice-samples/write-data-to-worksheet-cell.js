// Mirrors ONLYOFFICE's published macro sample "Write data to worksheet cell":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/write-data-to-worksheet-cell/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/write-data-to-worksheet-cell.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: C4 holds "Hello world".

(function () {
  Api.GetActiveSheet().GetRange("C4").SetValue("Hello world");
})();
