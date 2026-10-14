// Mirrors ONLYOFFICE's published macro sample "Change cell font color":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/change-cell-font-color/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/change-cell-font-color.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: B4's text is RGB(255, 0, 0).

(function () {
  var red = Api.CreateColorFromRGB(255, 0, 0);
  Api.GetActiveSheet().GetRange("B4").SetFontColor(red);
})();
