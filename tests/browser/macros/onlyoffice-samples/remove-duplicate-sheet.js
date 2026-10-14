// Mirrors ONLYOFFICE's published macro sample "Remove duplicate sheet":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/remove-duplicate-sheet/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/remove-duplicate-sheet.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: a sheet whose used range equals an earlier sheet's is deleted.

(function () {
  var sheets = Api.GetSheets();
  var same = function (a, b) {
    var x = a.GetUsedRange().GetValue(), y = b.GetUsedRange().GetValue();
    if (x.length !== y.length || x[0].length !== y[0].length) return false;
    for (var r = 0; r < x.length; r++)
      for (var c = 0; c < x[r].length; c++)
        if (a.GetRangeByNumber(r, c).GetValue() !== b.GetRangeByNumber(r, c).GetValue()) return false;
    return true;
  };
  var gone = {};
  for (var i = 0; i < sheets.length; i++) {
    if (gone[i]) continue;
    for (var j = i + 1; j < sheets.length; j++) {
      if (!gone[j] && same(sheets[i], sheets[j])) { sheets[j].Delete(); gone[j] = true; }
    }
  }
})();
