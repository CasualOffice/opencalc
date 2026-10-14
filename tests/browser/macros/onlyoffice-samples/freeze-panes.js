// Mirrors ONLYOFFICE's published macro sample "Freeze panes":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/freeze-panes/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/freeze-panes.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: panes are frozen so that A1:D10 is the frozen top-left block.

(function () {
  var sheet = Api.GetActiveSheet();
  sheet.GetFreezePanes().Unfreeze();
  sheet.GetRange("A10").Select();
  sheet.GetFreezePanes().FreezeAt(sheet.GetRange("D10"));
})();
