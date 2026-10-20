//! Clipboard fallback (D011, D043): copy the selection with a simulated key press, read
//! the text, and put the user's clipboard back.
//!
//! 1. Wait until the hotkey's modifiers are released (otherwise Ctrl+C would arrive as
//!    e.g. Ctrl+Alt+C), for up to [`MODIFIER_WAIT`]; still held after that →
//!    [`Unavailable::KeysHeld`] (D043).
//! 2. Save every clipboard format that holds plain memory ([`is_restorable_format`]).
//!    Formats that are GDI handles (bitmaps, metafiles, palettes) are skipped; the system
//!    synthesizes them from the memory formats (e.g. `CF_BITMAP` from `CF_DIB`).
//! 3. Send Ctrl+C, or Ctrl+Insert to terminals ([`copy_keys`]).
//! 4. Wait for the clipboard sequence number to change; no change means nothing was
//!    copied, i.e. no selection.
//! 5. Read `CF_UNICODETEXT`. A VS Code copy marked "from empty selection" (it then copies
//!    the whole line) counts as no selection.
//! 6. Put the saved formats back, marked with `ExcludeClipboardContentFromMonitorProcessing`,
//!    `CanIncludeInClipboardHistory = 0` and `CanUploadToCloudClipboard = 0`, so the
//!    restore does not add a clipboard-history entry. The entry created by the target's
//!    own copy in step 3 cannot be avoided (known limitation).

use std::mem::size_of;
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardOwner,
    GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput,
    VIRTUAL_KEY, VK_C, VK_CONTROL, VK_INSERT, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_MESSAGE, IsHungAppWindow, WINDOW_EX_STYLE, WINDOW_STYLE,
};
use windows::core::{PCWSTR, w};

use crate::clipboard_rules::{
    CHROMIUM_CUSTOM_MIME_FORMAT, CopyKeys, MAX_SAVED_BYTES, confirm_no_selection, copy_keys,
    is_restorable_format, is_vscode_empty_selection_copy,
};
use crate::{
    CaptureError, CaptureMethod, CaptureOutcome, CaptureProgress, CaptureStep, CaptureTarget,
    Progress, Selection, StepContext, StepOutput, Unavailable,
};

/// How long to wait for the user to release the hotkey's modifiers (D043).
const MODIFIER_WAIT: Duration = Duration::from_secs(3);
/// How long the target has to put the copy on the clipboard.
const COPY_WAIT: Duration = Duration::from_millis(400);
/// Retrying `OpenClipboard` while another program holds it.
const OPEN_RETRIES: u32 = 20;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(5);
const POLL: Duration = Duration::from_millis(5);

pub(super) const CF_UNICODETEXT: u32 = 13;

pub struct ClipboardStep {
    exclude_from_monitors: u32,
    can_include_in_history: u32,
    can_upload_to_cloud: u32,
    chromium_custom_mime: u32,
}

impl ClipboardStep {
    pub fn new() -> Self {
        // SAFETY: registering format names; the ids are stable for the session.
        unsafe {
            Self {
                exclude_from_monitors: RegisterClipboardFormatW(w!(
                    "ExcludeClipboardContentFromMonitorProcessing"
                )),
                can_include_in_history: RegisterClipboardFormatW(w!(
                    "CanIncludeInClipboardHistory"
                )),
                can_upload_to_cloud: RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")),
                chromium_custom_mime: register(CHROMIUM_CUSTOM_MIME_FORMAT),
            }
        }
    }
}

pub(super) fn register(name: &str) -> u32 {
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call.
    unsafe { RegisterClipboardFormatW(PCWSTR(wide.as_ptr())) }
}

impl CaptureStep for ClipboardStep {
    fn name(&self) -> &'static str {
        "clipboard"
    }

    fn confirms_no_selection(&self, target: &CaptureTarget, _context: &StepContext) -> bool {
        target.window.as_ref().is_some_and(confirm_no_selection)
    }

    fn try_capture(
        &self,
        target: &CaptureTarget,
        context: &StepContext,
        progress: Progress<'_>,
    ) -> StepOutput {
        let Some(window) = &target.window else {
            return unavailable(Unavailable::NoForegroundWindow, None);
        };
        let keys = copy_keys(window, context);
        let mut output = self.copy_selection(keys, progress);
        if keys == CopyKeys::CtrlInsert && output.note.is_none() {
            output.note = Some("ctrl_insert");
        }
        output
    }
}

fn unavailable(u: Unavailable, note: Option<&'static str>) -> StepOutput {
    StepOutput {
        note,
        ..StepOutput::from(Ok(CaptureOutcome::Unavailable(u)))
    }
}

fn platform(operation: &'static str, e: &windows::core::Error) -> CaptureError {
    CaptureError::Platform {
        operation,
        code: e.code().0 as u32,
        message: e.message(),
    }
}

pub(super) enum Save {
    Saved(Vec<(u32, Vec<u8>)>),
    TooLarge,
    OwnerHung,
}

impl ClipboardStep {
    fn copy_selection(&self, keys: CopyKeys, progress: Progress<'_>) -> StepOutput {
        let start = Instant::now();
        let released = wait_modifiers_released(MODIFIER_WAIT, progress);
        let key_wait = Some(start.elapsed());
        let output = if released {
            self.copy_released(keys)
        } else {
            unavailable(Unavailable::KeysHeld, None)
        };
        StepOutput { key_wait, ..output }
    }

    /// Copies once the hotkey's keys are up.
    fn copy_released(&self, keys: CopyKeys) -> StepOutput {
        let owner = match MessageWindow::new() {
            Ok(w) => w,
            Err(e) => return StepOutput::from(Err(e)),
        };
        let saved = match save(owner.0) {
            Ok(Save::Saved(saved)) => saved,
            Ok(Save::TooLarge) => {
                return unavailable(Unavailable::Unsupported, Some("clipboard_too_large"));
            }
            Ok(Save::OwnerHung) => {
                return unavailable(Unavailable::Unsupported, Some("clipboard_owner_hung"));
            }
            Err(e) => return StepOutput::from(Err(e)),
        };

        // SAFETY: plain Win32 call.
        let before = unsafe { GetClipboardSequenceNumber() };
        if let Err(e) = send_copy_keys(keys) {
            return StepOutput::from(Err(e));
        }
        if !wait_sequence_change(before, COPY_WAIT) {
            // Nothing was copied (or the program ignores the keys); the clipboard is as it was.
            return unavailable(Unavailable::NoSelection, Some("clipboard_unchanged"));
        }

        let read = self.read_copied_text(owner.0);
        // Always put the user's clipboard back once the target has replaced it.
        let restored = self.restore(owner.0, &saved);
        let output = match read {
            Ok(Copied::Text(text)) if !text.trim().is_empty() => {
                StepOutput::from(Ok(CaptureOutcome::Captured(Selection {
                    text,
                    context: None,
                    url: None,
                    method: CaptureMethod::Clipboard,
                })))
            }
            Ok(Copied::Text(_)) | Ok(Copied::None) => {
                unavailable(Unavailable::NoSelection, Some("clipboard_no_text"))
            }
            Ok(Copied::VscodeEmptySelection) => {
                unavailable(Unavailable::NoSelection, Some("vscode_empty_selection"))
            }
            Err(e) => StepOutput::from(Err(e)),
        };
        match restored {
            Ok(()) => output,
            // A failed restore is worth reporting even if the text was read.
            Err(e) => {
                eprintln!("capture: could not restore the clipboard: {e}");
                StepOutput {
                    note: Some("restore_failed"),
                    ..output
                }
            }
        }
    }

    fn read_copied_text(&self, owner: HWND) -> Result<Copied, CaptureError> {
        let _open = OpenGuard::open(owner)?;
        // SAFETY: the clipboard is open; handles are only used while it stays open.
        unsafe {
            if let Some(data) = read_format(self.chromium_custom_mime)
                && is_vscode_empty_selection_copy(&utf16_lossy(&data))
            {
                return Ok(Copied::VscodeEmptySelection);
            }
            Ok(match read_format(CF_UNICODETEXT) {
                Some(bytes) => Copied::Text(utf16_until_nul(&bytes)),
                None => Copied::None,
            })
        }
    }

    fn restore(&self, owner: HWND, saved: &[(u32, Vec<u8>)]) -> Result<(), CaptureError> {
        let _open = OpenGuard::open(owner)?;
        // SAFETY: the clipboard is open and owned by `owner`.
        unsafe {
            EmptyClipboard().map_err(|e| platform("EmptyClipboard", &e))?;
            for (format, bytes) in saved {
                put(*format, bytes)?;
            }
            // Keep the restore out of clipboard history, cloud clipboard and monitors.
            put(self.exclude_from_monitors, &[0])?;
            put(self.can_include_in_history, &0u32.to_ne_bytes())?;
            put(self.can_upload_to_cloud, &0u32.to_ne_bytes())?;
        }
        Ok(())
    }
}

enum Copied {
    Text(String),
    None,
    VscodeEmptySelection,
}

/// Reports [`CaptureProgress::WaitingForKeys`] when it has to wait, then
/// [`CaptureProgress::KeysReleased`] once the keys are up.
fn wait_modifiers_released(timeout: Duration, progress: Progress<'_>) -> bool {
    let deadline = Instant::now() + timeout;
    let mut waiting = false;
    loop {
        // SAFETY: plain Win32 calls. The high bit is set while a key is down.
        let held = [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
            .iter()
            .any(|vk| unsafe { GetAsyncKeyState(i32::from(vk.0)) } < 0);
        if !held {
            if waiting {
                progress(CaptureProgress::KeysReleased);
            }
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        if !waiting {
            waiting = true;
            progress(CaptureProgress::WaitingForKeys);
        }
        thread::sleep(POLL);
    }
}

fn wait_sequence_change(before: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        // SAFETY: plain Win32 call.
        if unsafe { GetClipboardSequenceNumber() } != before {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(POLL);
    }
}

fn send_copy_keys(keys: CopyKeys) -> Result<(), CaptureError> {
    let (key, extended) = match keys {
        CopyKeys::CtrlC => (VK_C, false),
        CopyKeys::CtrlInsert => (VK_INSERT, true),
    };
    let input = |vk: VIRTUAL_KEY, up: bool, extended: bool| {
        let mut flags = KEYBD_EVENT_FLAGS(0);
        if up {
            flags |= KEYEVENTF_KEYUP;
        }
        if extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    // With scan codes, like a real keyboard (browsers report `code`).
                    // SAFETY: plain Win32 call.
                    wScan: unsafe { MapVirtualKeyW(u32::from(vk.0), MAPVK_VK_TO_VSC) } as u16,
                    dwFlags: flags,
                    ..Default::default()
                },
            },
        }
    };
    let inputs = [
        input(VK_CONTROL, false, false),
        input(key, false, extended),
        input(key, true, extended),
        input(VK_CONTROL, true, false),
    ];
    // SAFETY: `inputs` is a valid slice of keyboard INPUTs.
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(CaptureError::Platform {
            operation: "SendInput",
            code: 0,
            message: format!("delivered {sent} of {} key events", inputs.len()),
        })
    }
}

/// Saves the clipboard's memory formats.
pub(super) fn save(owner: HWND) -> Result<Save, CaptureError> {
    if owner_hung() {
        return Ok(Save::OwnerHung);
    }
    let _open = OpenGuard::open(owner)?;
    let mut saved = Vec::new();
    let mut total = 0usize;
    let mut format = 0;
    // SAFETY: the clipboard is open; handles are only used while it stays open.
    unsafe {
        loop {
            format = EnumClipboardFormats(format);
            if format == 0 {
                break;
            }
            if !is_restorable_format(format) {
                continue;
            }
            let Some(bytes) = read_format(format) else {
                continue;
            };
            total += bytes.len();
            if total > MAX_SAVED_BYTES {
                return Ok(Save::TooLarge);
            }
            saved.push((format, bytes));
        }
    }
    Ok(Save::Saved(saved))
}

/// Reading a format the owner renders on demand makes Windows ask the owner for it, and
/// waits for the answer: never do that to a hung program.
pub(super) fn owner_hung() -> bool {
    // SAFETY: plain Win32 calls.
    unsafe { GetClipboardOwner() }
        .is_ok_and(|owner| !owner.is_invalid() && unsafe { IsHungAppWindow(owner) }.as_bool())
}

/// The bytes of one clipboard format, if it is present and holds memory.
///
/// # Safety
/// The clipboard must be open.
pub(super) unsafe fn read_format(format: u32) -> Option<Vec<u8>> {
    unsafe {
        let handle = GetClipboardData(format).ok()?;
        let global = HGLOBAL(handle.0);
        let size = GlobalSize(global);
        if size == 0 {
            return None;
        }
        let ptr = GlobalLock(global).cast::<u8>();
        if ptr.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
        let _ = GlobalUnlock(global);
        Some(bytes)
    }
}

/// Puts `bytes` on the clipboard as `format`.
///
/// # Safety
/// The clipboard must be open and emptied by us.
pub(super) unsafe fn put(format: u32, bytes: &[u8]) -> Result<(), CaptureError> {
    unsafe {
        let global = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1))
            .map_err(|e| platform("GlobalAlloc", &e))?;
        let ptr = GlobalLock(global).cast::<u8>();
        if ptr.is_null() {
            let _ = GlobalFree(Some(global));
            return Err(CaptureError::Platform {
                operation: "GlobalLock",
                code: 0,
                message: "null pointer".into(),
            });
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        let _ = GlobalUnlock(global);
        // On success the system owns the memory; on failure it is still ours.
        if let Err(e) = SetClipboardData(format, Some(HANDLE(global.0))) {
            let _ = GlobalFree(Some(global));
            return Err(platform("SetClipboardData", &e));
        }
    }
    Ok(())
}

pub(super) fn utf16_until_nul(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_ne_bytes(pair))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// All UTF-16 units of `bytes`, NULs included: Chromium's custom MIME pickle mixes binary
/// length fields with UTF-16 strings, and only the strings matter here.
pub(super) fn utf16_lossy(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_ne_bytes(pair))
        .collect();
    String::from_utf16_lossy(&units)
}

/// Holds the clipboard open; closes it on drop.
pub(super) struct OpenGuard;

impl OpenGuard {
    pub(super) fn open(owner: HWND) -> Result<Self, CaptureError> {
        let mut last = None;
        for _ in 0..OPEN_RETRIES {
            // SAFETY: plain Win32 call; balanced by CloseClipboard in Drop.
            match unsafe { OpenClipboard(Some(owner)) } {
                Ok(()) => return Ok(Self),
                Err(e) => last = Some(e),
            }
            thread::sleep(OPEN_RETRY_DELAY);
        }
        Err(last.map_or_else(
            || CaptureError::WorkerUnavailable("clipboard busy".into()),
            |e| platform("OpenClipboard", &e),
        ))
    }
}

impl Drop for OpenGuard {
    fn drop(&mut self) {
        // SAFETY: the clipboard was opened by this guard.
        let _ = unsafe { CloseClipboard() };
    }
}

/// A message-only window to own the clipboard while it is restored: `SetClipboardData`
/// fails when the clipboard was opened without a window.
pub(super) struct MessageWindow(pub(super) HWND);

impl MessageWindow {
    pub(super) fn new() -> Result<Self, CaptureError> {
        // SAFETY: creates a message-only window of a system class; destroyed on drop.
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("LexTrail clipboard"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
            .map(Self)
            .map_err(|e| platform("CreateWindowExW", &e))
        }
    }
}

impl Drop for MessageWindow {
    fn drop(&mut self) {
        // SAFETY: the window was created by this thread.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf16_text_up_to_nul() {
        let mut bytes: Vec<u8> = "héllo".encode_utf16().flat_map(u16::to_ne_bytes).collect();
        bytes.extend([0, 0, b'x', 0]);
        assert_eq!(utf16_until_nul(&bytes), "héllo");
    }

    #[test]
    fn finds_vscode_marker_in_a_chromium_pickle() {
        // Binary length fields followed by the UTF-16 type and JSON strings.
        let utf16 = |s: &str| {
            s.encode_utf16()
                .flat_map(u16::to_ne_bytes)
                .collect::<Vec<u8>>()
        };
        let mut bytes = vec![0x40, 0, 0, 0, 1, 0, 0, 0, 18, 0, 0, 0];
        bytes.extend(utf16("vscode-editor-data"));
        bytes.extend([30, 0, 0, 0]);
        bytes.extend(utf16(r#"{"isFromEmptySelection":true}"#));
        assert!(is_vscode_empty_selection_copy(&utf16_lossy(&bytes)));
    }

    /// Round trip through the real clipboard. Ignored by default because it changes the
    /// clipboard of whoever runs the tests (it puts the original back at the end):
    /// `cargo test -p lextrail-capture -- --ignored clipboard_round_trip`.
    #[test]
    #[ignore]
    fn clipboard_round_trip() {
        let step = ClipboardStep::new();
        let owner = MessageWindow::new().unwrap();
        let Save::Saved(original) = save(owner.0).unwrap() else {
            panic!("could not save the current clipboard");
        };

        let custom = register("LexTrail test format");
        let text: Vec<u8> = "round trip 测试"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_ne_bytes)
            .collect();
        let test_content = vec![(CF_UNICODETEXT, text.clone()), (custom, vec![1, 2, 3, 4])];
        step.restore(owner.0, &test_content).unwrap();

        let Save::Saved(saved) = save(owner.0).unwrap() else {
            panic!("could not save the test clipboard");
        };
        let restored_ok = step.restore(owner.0, &original);

        let find = |f: u32| saved.iter().find(|(g, _)| *g == f).map(|(_, b)| b.clone());
        assert_eq!(find(custom).as_deref(), Some(&[1u8, 2, 3, 4][..]));
        assert_eq!(
            utf16_until_nul(&find(CF_UNICODETEXT).unwrap()),
            "round trip 测试"
        );
        // The history markers are on the clipboard too.
        assert_eq!(
            find(step.can_include_in_history).as_deref(),
            Some(&0u32.to_ne_bytes()[..])
        );
        restored_ok.unwrap();
    }
}
