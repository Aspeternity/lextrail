//! Platform-independent rules for the clipboard fallback (D011, D043): which keys copy,
//! which targets get their "no selection" double-checked, which clipboard formats can be
//! saved and put back, and how to spot a copy that was not of a selection.

use crate::{StepContext, WindowInfo};

/// The key combination that copies the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyKeys {
    CtrlC,
    /// For terminals, where Ctrl+C interrupts the running program when nothing is selected.
    CtrlInsert,
}

/// Window classes of terminals (Ctrl+C would send an interrupt).
const TERMINAL_CLASSES: &[&str] = &[
    "CASCADIA_HOSTING_WINDOW_CLASS", // Windows Terminal
    "ConsoleWindowClass",            // conhost
    "PseudoConsoleWindow",
    "mintty",              // Git Bash, Cygwin
    "VirtualConsoleClass", // ConEmu
];

/// Class of the focused element of VS Code's (xterm.js) integrated terminal.
const XTERM_FOCUS_CLASS: &str = "xterm-helper-textarea";

/// Chromium-based windows (Chrome, Edge, Electron apps such as VS Code).
pub fn is_chromium_class(class: &str) -> bool {
    class.starts_with("Chrome_WidgetWin_")
}

pub fn is_terminal(window: &WindowInfo, context: &StepContext) -> bool {
    window
        .class_name
        .as_deref()
        .is_some_and(|c| TERMINAL_CLASSES.contains(&c))
        || context
            .focused_class
            .as_deref()
            .is_some_and(|c| c.split_whitespace().any(|part| part == XTERM_FOCUS_CLASS))
}

/// The keys to send to copy the selection of `window`.
pub fn copy_keys(window: &WindowInfo, context: &StepContext) -> CopyKeys {
    if is_terminal(window, context) {
        CopyKeys::CtrlInsert
    } else {
        CopyKeys::CtrlC
    }
}

/// Whether a "no selection" from UI Automation should be double-checked with the
/// clipboard. Only for targets where UIA is known to miss selections (D043): Chromium
/// windows (VS Code's editor and terminal, a PDF that just opened in Chrome). Programs
/// whose UIA answer is reliable (Notepad, Word, Windows Terminal) are not copied from, to
/// save the delay and the clipboard round trip.
pub fn confirm_no_selection(window: &WindowInfo) -> bool {
    window.class_name.as_deref().is_some_and(is_chromium_class)
}

/// Standard clipboard formats whose data is a GDI handle rather than memory, so they
/// cannot be saved as bytes. The system synthesizes most of them from formats that can
/// (e.g. `CF_BITMAP` from `CF_DIB`), so nothing visible is lost.
const HANDLE_FORMATS: &[u32] = &[
    2,    // CF_BITMAP
    3,    // CF_METAFILEPICT
    9,    // CF_PALETTE
    14,   // CF_ENHMETAFILE
    0x80, // CF_OWNERDISPLAY
    0x82, // CF_DSPBITMAP
    0x83, // CF_DSPMETAFILEPICT
    0x8E, // CF_DSPENHMETAFILE
];

/// Private GDI object formats (`CF_GDIOBJFIRST`..=`CF_GDIOBJLAST`).
const GDI_OBJECT_FORMATS: std::ops::RangeInclusive<u32> = 0x0300..=0x03FF;

/// Whether a clipboard format holds plain memory that can be saved and restored.
pub fn is_restorable_format(format: u32) -> bool {
    !HANDLE_FORMATS.contains(&format) && !GDI_OBJECT_FORMATS.contains(&format)
}

/// Above this total size the clipboard is not touched: copying it would be slow, and a
/// failed restore would lose a lot.
pub const MAX_SAVED_BYTES: usize = 64 * 1024 * 1024;

/// VS Code writes this format next to the text it copies.
pub const VSCODE_EDITOR_DATA: &str = "vscode-editor-data";

/// Whether VS Code's `vscode-editor-data` says the copy came from an empty selection (VS
/// Code then copies the whole line, which is not what the user selected).
pub fn is_vscode_empty_selection_copy(editor_data: &str) -> bool {
    let compact: String = editor_data.chars().filter(|c| !c.is_whitespace()).collect();
    compact.contains("\"isFromEmptySelection\":true")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(class: &str) -> WindowInfo {
        WindowInfo {
            class_name: Some(class.to_owned()),
            ..WindowInfo::for_tests("app.exe")
        }
    }

    fn focused(class: &str) -> StepContext {
        StepContext {
            focused_class: Some(class.to_owned()),
            ..StepContext::default()
        }
    }

    #[test]
    fn terminals_use_ctrl_insert() {
        let none = StepContext::default();
        assert_eq!(
            copy_keys(&window("CASCADIA_HOSTING_WINDOW_CLASS"), &none),
            CopyKeys::CtrlInsert
        );
        assert_eq!(
            copy_keys(&window("ConsoleWindowClass"), &none),
            CopyKeys::CtrlInsert
        );
        assert_eq!(copy_keys(&window("Notepad"), &none), CopyKeys::CtrlC);
        // VS Code: Ctrl+C in the editor, Ctrl+Insert in the integrated terminal.
        let code = window("Chrome_WidgetWin_1");
        assert_eq!(
            copy_keys(&code, &focused("monaco-mouse-cursor-text")),
            CopyKeys::CtrlC
        );
        assert_eq!(
            copy_keys(&code, &focused("xterm-helper-textarea")),
            CopyKeys::CtrlInsert
        );
        assert_eq!(
            copy_keys(&code, &focused("foo xterm-helper-textarea bar")),
            CopyKeys::CtrlInsert
        );
    }

    #[test]
    fn only_chromium_windows_get_no_selection_confirmed() {
        assert!(confirm_no_selection(&window("Chrome_WidgetWin_1")));
        assert!(!confirm_no_selection(&window("Notepad")));
        assert!(!confirm_no_selection(&window("OpusApp"))); // Word
        assert!(!confirm_no_selection(&window(
            "CASCADIA_HOSTING_WINDOW_CLASS"
        )));
        assert!(!confirm_no_selection(&WindowInfo::for_tests("app.exe")));
    }

    #[test]
    fn handle_formats_are_not_restorable() {
        assert!(is_restorable_format(13)); // CF_UNICODETEXT
        assert!(is_restorable_format(1)); // CF_TEXT
        assert!(is_restorable_format(8)); // CF_DIB
        assert!(is_restorable_format(15)); // CF_HDROP
        assert!(is_restorable_format(0xC123)); // registered, e.g. "HTML Format"
        assert!(!is_restorable_format(2)); // CF_BITMAP
        assert!(!is_restorable_format(14)); // CF_ENHMETAFILE
        assert!(!is_restorable_format(0x0300));
        assert!(!is_restorable_format(0x03FF));
    }

    #[test]
    fn vscode_empty_selection_marker() {
        assert!(is_vscode_empty_selection_copy(
            r#"{"version":1,"isFromEmptySelection":true,"multicursorText":null,"mode":"markdown"}"#
        ));
        assert!(is_vscode_empty_selection_copy(
            r#"{ "isFromEmptySelection" : true }"#
        ));
        assert!(!is_vscode_empty_selection_copy(
            r#"{"version":1,"isFromEmptySelection":false,"mode":"markdown"}"#
        ));
        assert!(!is_vscode_empty_selection_copy(""));
    }
}
