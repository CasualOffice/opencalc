// Mirrors ONLYOFFICE's published macro sample "Check data mismatch":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/check-data-mismatch/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/check-data-mismatch.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: below the header row and right of the label column, a cell whose type differs from its column's majority is filled RGB(255, 213, 191).

(function () {
  var sheet = Api.GetActiveSheet();
  var corner = sheet.GetUsedRange().GetAddress().split(":")[1];
  var data = sheet.GetRange("B2:" + corner).GetValue();
  if (!data || !data.length) return;
  var isNum = function (v) { return !isNaN(parseFloat(v)); };
  var letters = function (n) {
    var s = "";
    while (n > 0) { var m = (n - 1) % 26; s = String.fromCharCode(65 + m) + s; n = Math.floor((n - m) / 26); }
    return s;
  };
  for (var j = 0; j < data[0].length; j++) {
    var nums = 0, texts = 0;
    for (var i = 0; i < data.length; i++) {
      if (data[i][j] === "" || data[i][j] === null) continue;
      if (isNum(data[i][j])) nums++; else texts++;
    }
    var wantNumber = nums >= texts;
    for (var k = 0; k < data.length; k++) {
      var v = data[k][j];
      if (v === "" || v === null) continue;
      if (isNum(v) !== wantNumber) {
        sheet.GetRange(letters(j + 2) + (k + 2)).SetFillColor(Api.CreateColorFromRGB(255, 213, 191));
      }
    }
  }
})();
