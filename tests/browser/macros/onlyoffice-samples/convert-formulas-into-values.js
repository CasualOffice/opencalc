// Mirrors ONLYOFFICE's published macro sample "Convert formulas into values":
//   https://api.onlyoffice.com/docs/macros/samples/spreadsheet-editor/convert-formulas-into-values/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com/blob/master/site/docs/macros/samples/spreadsheet-editor/convert-formulas-into-values.md)
// Written here against the same API calls rather than copied: the samples are
// GPL-3.0 and this repository is Apache-2.0.
//
// Documented result: every formula in the used range is replaced by its computed value.

(function () {
  var used = Api.GetActiveSheet().GetUsedRange();
  if (!used) return;
  // GetValue evaluates; writing the values back drops the formulas.
  var computed = used.GetValue();
  used.SetValue(computed);
})();
