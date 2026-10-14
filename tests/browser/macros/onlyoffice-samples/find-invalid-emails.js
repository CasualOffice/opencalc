// Mirrors ONLYOFFICE's published macro sample "Find invalid emails":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/find-invalid-emails/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/find-invalid-emails.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: cells in column B (0-based column 1) that are not e-mail addresses are filled RGB(255, 163, 163).

(function () {
  var sheet = Api.GetActiveSheet();
  var used = sheet.GetUsedRange();
  var pattern = /^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$/;
  var highlight = Api.CreateColorFromRGB(255, 163, 163);
  var column = 1;
  var rows = used.GetRows().Count;
  for (var r = 0; r < rows; r++) {
    var cell = sheet.GetRangeByNumber(r, column);
    var text = cell.GetValue();
    if (!text) continue;
    text = text.toString().trim();
    if (text && !pattern.test(text)) cell.SetFillColor(highlight);
  }
})();
