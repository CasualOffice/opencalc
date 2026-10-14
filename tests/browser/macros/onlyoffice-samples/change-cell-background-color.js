// Mirrors ONLYOFFICE's published macro sample "Change cell background color":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/change-cell-background-color/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/change-cell-background-color.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: B3 is filled RGB(0, 0, 250).

(function () {
  var blue = Api.CreateColorFromRGB(0, 0, 250);
  Api.GetActiveSheet().GetRange("B3").SetFillColor(blue);
})();
