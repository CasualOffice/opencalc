//! The desktop shell's backend: what a Tauri command calls.
//!
//! [`ADR-023`](../../docs/81-DESKTOP-SHELL-COMPOSITION.md) settled that there is
//! no crate between the app and `casual-calc-sdk`. This is the app's own thin
//! layer over that SDK — the part a `#[tauri::command]` wraps, kept separate
//! from Tauri itself so it can be tested without a webview.
//!
//! **Tauri is deliberately not a dependency yet.** Every function here is the
//! shape a command takes: bytes and plain values in, JSON out. Adding
//! `#[tauri::command]` is an attribute, not a redesign — and keeping the
//! dependency out until the shell needs a window means this half is provable by
//! `cargo test` rather than by launching an application.
//!
//! The division of labour is [44](../../docs/44-TAURI-DESKTOP-SHELL-DESIGN.md)'s:
//! **the shell renders a display list, it is not the source of truth.** Nothing
//! here decides where anything is drawn; it forwards a viewport and returns what
//! the engine says goes in it. `RND-10` made the browser editor work exactly
//! this way, so the webview half of this app is code that already exists.

pub mod assets;
pub mod dialog;
pub mod menu;
pub mod save;
pub mod session;
pub mod title;

use casual_calc_layout::Viewport;
use casual_calc_sdk::WorkbookSession;

/// A workbook open in the shell, and the format it came from.
///
/// One session per window. The shell owns it; the webview never sees a model,
/// only display lists and the values it asked for.
pub struct Desktop {
    session: WorkbookSession,
}

/// What went wrong, in terms a shell can show somebody.
#[derive(Debug)]
pub enum ShellError {
    /// The bytes were not a workbook this can open.
    Open(String),
    /// The workbook could not be written back.
    Save(String),
    /// A sheet index that does not name a sheet.
    NoSuchSheet(usize),
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open(why) => write!(f, "could not open: {why}"),
            Self::Save(why) => write!(f, "could not save: {why}"),
            Self::NoSuchSheet(i) => write!(f, "no sheet {i}"),
        }
    }
}

impl std::error::Error for ShellError {}

impl Desktop {
    /// Open a workbook from bytes.
    ///
    /// **Bytes, not a path.** The shell reads the file and the engine is handed
    /// its contents — [44](../../docs/44-TAURI-DESKTOP-SHELL-DESIGN.md) states
    /// it as an invariant: *the engine receives bytes, never raw paths to fetch
    /// on its own*. A path crossing this line would put file access inside the
    /// part of the system that parses untrusted input.
    pub fn open(bytes: Vec<u8>) -> Result<Self, ShellError> {
        WorkbookSession::open(bytes)
            .map(|session| Self { session })
            .map_err(|why| ShellError::Open(why.to_string()))
    }

    /// A new window's workbook: one empty sheet.
    ///
    /// `with_sheet`, not `blank` plus a sheet added here. The engine now knows
    /// that an interactive host wants a sheet to open on, so this host does not
    /// have to (`SDK-011`).
    pub fn blank() -> Self {
        Self {
            session: WorkbookSession::with_sheet(),
        }
    }

    /// What to paint for `viewport`, as JSON.
    ///
    /// The whole rendering contract, and it is one call. The viewport is in the
    /// layout's own units — twips — because that is what the engine works in;
    /// converting at the boundary is what `RND-10` got wrong first, drawing a
    /// chart's frame and nothing inside it, so the unit belongs in the type's
    /// documentation rather than in a caller's memory.
    pub fn frame(&self, sheet: usize, viewport: &Viewport) -> Result<String, ShellError> {
        if sheet >= self.sheet_count() {
            return Err(ShellError::NoSuchSheet(sheet));
        }
        let list = self.session.layout(sheet, viewport);
        serde_json::to_string(&list).map_err(|why| ShellError::Open(why.to_string()))
    }

    /// Type into a cell.
    ///
    /// Through `WorkbookSession::input_edit`, which is the *same function* the
    /// browser calls — not a desktop reimplementation of what typed text means.
    /// That rule (when an entry is a formula, when a leading apostrophe forces
    /// text, which number format `007` implies) lived only in the WebAssembly
    /// bridge until `TAURI-002` moved it here, and a second host was the thing
    /// that would have had to guess at it.
    pub fn set_cell(
        &mut self,
        sheet: usize,
        row: u32,
        col: u32,
        input: &str,
    ) -> Result<(), ShellError> {
        let at = casual_calc_sdk::CellRef::new(row, col);
        let op = self.session.input_edit(sheet, at, input);
        self.session
            .edit(op)
            .map_err(|why| ShellError::Open(why.to_string()))
    }

    /// What a cell would show, so a shell can fill its formula bar.
    pub fn cell_input(&self, sheet: usize, row: u32, col: u32) -> String {
        self.session
            .cell_input(sheet, casual_calc_sdk::CellRef::new(row, col))
    }

    /// Take back the last edit.
    pub fn undo(&mut self) -> Result<(), ShellError> {
        self.session
            .undo()
            .map_err(|why| ShellError::Open(why.to_string()))
    }

    /// The workbook as `.xlsx` bytes, for the shell to write where it likes.
    pub fn save(&mut self) -> Result<Vec<u8>, ShellError> {
        self.session
            .save()
            .map_err(|why| ShellError::Save(why.to_string()))
    }

    /// How many sheets there are, so a shell can draw its tab strip.
    pub fn sheet_count(&self) -> usize {
        self.session.workbook().sheets.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole shell loop, without a window.
    ///
    /// This is what `TAURI-001` is for: proving the backend half of
    /// [44](../../docs/44-TAURI-DESKTOP-SHELL-DESIGN.md)'s diagram works before
    /// a webview exists to look at it. Open, lay out, edit, undo, save.
    #[test]
    fn a_shell_can_open_edit_and_save_without_a_window() {
        let mut shell = Desktop::blank();
        assert_eq!(shell.sheet_count(), 1);

        shell.set_cell(0, 0, 0, "12").expect("typing a number");
        shell.set_cell(0, 0, 1, "=A1*2").expect("typing a formula");
        assert_eq!(
            shell.cell_input(0, 0, 1),
            "=A1*2",
            "a formula reads back as one"
        );

        // The rendering contract: a viewport in, a display list out. Twips,
        // because that is the engine's unit — the conversion `RND-10` got wrong
        // in the other direction.
        let frame = shell
            .frame(
                0,
                &Viewport {
                    x: 0,
                    y: 0,
                    width: 1_920 * 15,
                    height: 1_080 * 15,
                },
            )
            .expect("a frame for sheet 0");
        assert!(frame.contains("items"), "a display list came back: {frame}");

        shell.undo().expect("undo the formula");
        assert_eq!(shell.cell_input(0, 0, 1), "", "undo took the formula back");

        let bytes = shell.save().expect("saving");
        assert!(bytes.starts_with(b"PK"), "an .xlsx package");

        // And it reopens — a save nothing can read is not a save.
        let again = Desktop::open(bytes).expect("reopening what we wrote");
        assert_eq!(again.cell_input(0, 0, 0), "12");
    }

    /// A sheet index that names no sheet is refused, not clamped.
    #[test]
    fn a_frame_for_a_missing_sheet_is_an_error() {
        let shell = Desktop::blank();
        let v = Viewport {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        };
        assert!(matches!(
            shell.frame(9, &v),
            Err(ShellError::NoSuchSheet(9))
        ));
    }
}
