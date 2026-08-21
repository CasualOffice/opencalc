# Detection fixtures

Zip packages for `detect`, **written by Python's `zipfile`** rather than by
anything in this workspace. A fixture produced by the code that reads it proves
the code agrees with itself; these prove the local-file-header offsets are the
ones a real zip writer emits.

- `sheet.ods` — `mimetype` first and **stored**, as ODF requires so that a
  reader can identify the document without decompressing it.
- `text.odt` — the same shape with a different media type. An ODF document that
  is not a spreadsheet must not be opened as one.
- `book.xlsx` — `[Content_Types].xml` first, deflated, as OOXML writes it.
- `other.zip` — a zip that is neither, which must be refused rather than guessed
  at.
