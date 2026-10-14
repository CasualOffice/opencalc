// Mirrors ONLYOFFICE's published macro sample "Highlight duplicates":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/highlight-duplicates/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/highlight-duplicates.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: in the selection, each repeated value gets its own colour from the palette (first RGB(255, 255, 0), then RGB(204, 204, 255)); unique values are filled white.

(function () {
  var palette = [
    Api.CreateColorFromRGB(255, 255, 0),
    Api.CreateColorFromRGB(204, 204, 255),
    Api.CreateColorFromRGB(0, 255, 0),
  ];
  var white = Api.CreateColorFromRGB(255, 255, 255);
  var next = 0;
  var counts = {};
  var cells = [];
  Api.ActiveSheet.Selection.ForEach(function (cell) {
    var v = cell.GetValue();
    counts[v] = (counts[v] || 0) + 1;
    cells.push(cell);
  });
  var assigned = {};
  cells.forEach(function (cell) {
    var v = cell.GetValue();
    if (counts[v] > 1) {
      if (!(v in assigned)) assigned[v] = palette[next++ % palette.length];
      cell.SetFillColor(assigned[v]);
    } else {
      cell.SetFillColor(white);
    }
  });
})();
