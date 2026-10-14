// Mirrors ONLYOFFICE's published macro sample "Sum of highlighted cells":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/sum-of-highlighted-cells/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/sum-of-highlighted-cells.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: A17 reads "The sum: N", N being the sum of A1:A16 cells filled like B1.

(function () {
  var sheet = Api.GetActiveSheet();
  var reference = sheet.GetRange("B1");
  reference.SetFillColor(Api.CreateColorFromRGB(91, 155, 213));
  var target = reference.GetFillColor().GetRGB();
  var sum = 0;
  sheet.GetRange("A1:A16").ForEach(function (cell) {
    var fill = cell.GetFillColor();
    if (fill === "No Fill" || fill.GetRGB() !== target) return;
    var n = parseFloat(cell.GetValue());
    if (!isNaN(n)) sum += n;
  });
  sheet.GetRange("A17").SetValue("The sum: " + sum);
})();
