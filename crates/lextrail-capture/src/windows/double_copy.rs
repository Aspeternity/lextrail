//! Windows side of double Ctrl+C (D012, `docs/specs/m1-double-copy.md`): a message-only
//! window on its own thread receives `WM_CLIPBOARDUPDATE`, reads what each update holds
//! and feeds it to a [`DoubleCopyDetector`].
//!
//! Reading the clipboard (spec section 5): programs such as Excel render large copies only
//! when a format is read, so the clipboard is not opened at all for updates that would be
//! ignored anyway (LexTrail's own capture, its own window in front, the "do not monitor"
//! marker), the hung-owner check comes first, and the clipboard is closed as soon as the
//! text is copied out. Opening is retried for at most about 100 ms; an update that cannot
//! be read counts as "no text".

use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::thread;
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, IsClipboardFormatAvailable, RemoveClipboardFormatListener,
};
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, MSG,
    PostThreadMessageW, WM_CLIPBOARDUPDATE, WM_QUIT,
};

use super::clipboard::{
    CF_UNICODETEXT, MessageWindow, OpenGuard, owner_hung, read_format, register, utf16_lossy,
    utf16_until_nul,
};
use crate::clipboard_rules::{CHROMIUM_CUSTOM_MIME_FORMAT, is_vscode_empty_selection_copy};
use crate::{CaptureError, CopiedContent, DoubleCopy, DoubleCopyDetector, check_text};

/// Listens for double Ctrl+C until dropped. Start it only when the setting is on (no
/// listener, no cost, spec section 1).
pub struct DoubleCopyListener {
    detector: Arc<Mutex<DoubleCopyDetector>>,
    thread_id: u32,
}

impl DoubleCopyListener {
    /// Starts the listener thread. `on_double` runs on that thread for each double
    /// Ctrl+C; keep it short (hand the lookup to another thread).
    pub fn start(on_double: impl Fn(DoubleCopy) + Send + 'static) -> Result<Self, CaptureError> {
        let detector = Arc::new(Mutex::new(DoubleCopyDetector::new()));
        let shared = Arc::clone(&detector);
        let (ready, started) = mpsc::channel();
        thread::Builder::new()
            .name("lextrail-double-copy".into())
            .spawn(move || listen(&shared, &ready, &on_double))
            .map_err(|e| CaptureError::WorkerUnavailable(e.to_string()))?;
        let thread_id = started.recv().map_err(|_| {
            CaptureError::WorkerUnavailable("double-copy listener stopped".into())
        })??;
        Ok(Self {
            detector,
            thread_id,
        })
    }

    /// A hotkey capture `id` started now: its clipboard fallback's updates are ignored
    /// (spec section 2, rule 6).
    pub fn own_copy_started(&self, id: u64) {
        lock(&self.detector).own_copy_started(id, Instant::now());
    }

    /// Hotkey capture `id` finished now (clipboard restored).
    pub fn own_copy_finished(&self, id: u64) {
        lock(&self.detector).own_copy_finished(id, Instant::now());
    }
}

impl Drop for DoubleCopyListener {
    fn drop(&mut self) {
        // SAFETY: plain Win32 call; the thread removes its listener and window on WM_QUIT.
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
    }
}

/// Clipboard format ids registered by name.
struct Formats {
    exclude_from_monitors: u32,
    can_include_in_history: u32,
    chromium_custom_mime: u32,
}

/// What one update turned out to be.
enum Update {
    /// Not read: within LexTrail's own capture.
    Ignored,
    /// Not read: LexTrail's own window is in front.
    OwnWindow,
    /// Marked "do not monitor" or "not for the clipboard history" (spec section 2, rule 7):
    /// which of the two, for the debug log.
    Marked(#[cfg_attr(not(debug_assertions), allow(dead_code))] &'static str),
    Read(CopiedContent),
}

fn listen(
    detector: &Mutex<DoubleCopyDetector>,
    ready: &mpsc::Sender<Result<u32, CaptureError>>,
    on_double: &dyn Fn(DoubleCopy),
) {
    let window = match MessageWindow::new() {
        Ok(w) => w,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    // SAFETY: the window belongs to this thread and lives until the end of the function.
    if let Err(e) = unsafe { AddClipboardFormatListener(window.0) } {
        let _ = ready.send(Err(CaptureError::Platform {
            operation: "AddClipboardFormatListener",
            code: e.code().0 as u32,
            message: e.message(),
        }));
        return;
    }
    let formats = Formats {
        exclude_from_monitors: register("ExcludeClipboardContentFromMonitorProcessing"),
        can_include_in_history: register("CanIncludeInClipboardHistory"),
        chromium_custom_mime: register(CHROMIUM_CUSTOM_MIME_FORMAT),
    };
    // SAFETY: plain Win32 call.
    let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));

    let mut msg = MSG::default();
    // SAFETY: a plain message loop on this thread; WM_CLIPBOARDUPDATE is posted.
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        if msg.message == WM_CLIPBOARDUPDATE && msg.hwnd == window.0 {
            let at = Instant::now();
            let update = classify(detector, at, window.0, &formats);
            #[cfg(debug_assertions)]
            log_update(&update, at);
            let found = match update {
                Update::Ignored => None,
                Update::OwnWindow | Update::Marked(_) => {
                    lock(detector).discard();
                    None
                }
                Update::Read(content) => lock(detector).on_update(at, content),
            };
            match found {
                Some(DoubleCopy::Text(text)) => match check_text(&text) {
                    Ok(()) => on_double(DoubleCopy::Text(text)),
                    // Quietly ignored: no popup, no history; the reason only, never the text.
                    #[cfg(debug_assertions)]
                    Err(why) => eprintln!("double copy: ignored: {why:?}"),
                    #[cfg(not(debug_assertions))]
                    Err(_) => {}
                },
                Some(double) => on_double(double),
                None => {}
            }
        } else {
            // SAFETY: dispatching this thread's own message.
            unsafe { DispatchMessageW(&msg) };
        }
    }
    // SAFETY: the listener was added on this window above.
    let _ = unsafe { RemoveClipboardFormatListener(window.0) };
}

fn classify(
    detector: &Mutex<DoubleCopyDetector>,
    at: Instant,
    window: HWND,
    formats: &Formats,
) -> Update {
    if lock(detector).ignores(at) {
        return Update::Ignored;
    }
    if own_window_in_front() {
        return Update::OwnWindow;
    }
    // SAFETY: plain Win32 call; needs no open clipboard.
    if unsafe { IsClipboardFormatAvailable(formats.exclude_from_monitors) }.is_ok() {
        return Update::Marked("exclude from monitors");
    }
    if owner_hung() {
        return Update::Read(CopiedContent::Other);
    }
    read(window, formats)
}

/// Opens the clipboard only for as long as copying the data out takes.
fn read(window: HWND, formats: &Formats) -> Update {
    let Ok(_open) = OpenGuard::open(window) else {
        return Update::Read(CopiedContent::Other);
    };
    // SAFETY: the clipboard is open; handles are only used while it stays open.
    unsafe {
        if IsClipboardFormatAvailable(formats.can_include_in_history).is_ok()
            && read_format(formats.can_include_in_history)
                .is_some_and(|bytes| bytes.get(..4) == Some(&0u32.to_ne_bytes()[..]))
        {
            return Update::Marked("not for history");
        }
        let text = || read_format(CF_UNICODETEXT).map(|bytes| utf16_until_nul(&bytes));
        if IsClipboardFormatAvailable(formats.chromium_custom_mime).is_ok()
            && read_format(formats.chromium_custom_mime)
                .is_some_and(|data| is_vscode_empty_selection_copy(&utf16_lossy(&data)))
        {
            return Update::Read(text().map_or(CopiedContent::Other, |line| {
                CopiedContent::EmptySelectionLine(line)
            }));
        }
        Update::Read(text().map_or(CopiedContent::Other, CopiedContent::Text))
    }
}

/// A LexTrail window (the popup, the main window) is the foreground window.
fn own_window_in_front() -> bool {
    // SAFETY: plain Win32 calls.
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() {
            return false;
        }
        let mut process = 0;
        GetWindowThreadProcessId(foreground, Some(&mut process));
        process == GetCurrentProcessId()
    }
}

/// Debug builds: one line per update, without the text, with how long it took to read
/// (spec section 5: large Excel copies).
#[cfg(debug_assertions)]
fn log_update(update: &Update, at: Instant) {
    let kind = match update {
        Update::Ignored => "ignored (own capture)",
        Update::OwnWindow => "ignored (own window)",
        Update::Marked(_) => "ignored (marked)",
        Update::Read(CopiedContent::Text(_)) => "text",
        Update::Read(CopiedContent::EmptySelectionLine(_)) => "vscode empty selection",
        Update::Read(CopiedContent::Other) => "no text",
    };
    let marker = match update {
        Update::Marked(which) => format!(": {which}"),
        _ => String::new(),
    };
    eprintln!(
        "double copy: clipboard update: {kind}{marker} {:.1}ms",
        at.elapsed().as_secs_f64() * 1000.0
    );
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use windows::Win32::System::DataExchange::EmptyClipboard;

    use super::super::clipboard::{Save, put, save};
    use super::*;

    /// Replaces the clipboard with `formats`, as a program's copy would.
    fn write(owner: HWND, formats: &[(u32, Vec<u8>)]) {
        let _open = OpenGuard::open(owner).unwrap();
        // SAFETY: the clipboard is open and owned by `owner`.
        unsafe {
            EmptyClipboard().unwrap();
            for (format, bytes) in formats {
                put(*format, bytes).unwrap();
            }
        }
    }

    fn text(s: &str) -> (u32, Vec<u8>) {
        let bytes = s.encode_utf16().chain(Some(0)).flat_map(u16::to_ne_bytes);
        (CF_UNICODETEXT, bytes.collect())
    }

    /// Through the real clipboard and its notifications. Ignored by default because it
    /// changes the clipboard of whoever runs the tests (and its Win+V history; the
    /// original content is put back at the end):
    /// `cargo test -p lextrail-capture -- --ignored listener_round_trip`.
    #[test]
    #[ignore]
    fn listener_round_trip() {
        let owner = MessageWindow::new().unwrap();
        let Save::Saved(original) = save(owner.0).unwrap() else {
            panic!("could not save the current clipboard");
        };
        let (found, doubles) = mpsc::channel();
        let listener = DoubleCopyListener::start(move |d| found.send(d).unwrap()).unwrap();
        let copy_twice = |formats: &[(u32, Vec<u8>)]| {
            write(owner.0, formats);
            thread::sleep(Duration::from_millis(150));
            write(owner.0, formats);
            doubles.recv_timeout(Duration::from_millis(500)).ok()
        };
        let pause = || thread::sleep(Duration::from_millis(700));

        let plain = copy_twice(&[text("listener test")]);
        pause();
        let exclude = register("ExcludeClipboardContentFromMonitorProcessing");
        let excluded = copy_twice(&[text("listener test"), (exclude, vec![0])]);
        pause();
        let history = register("CanIncludeInClipboardHistory");
        let no_history = copy_twice(&[
            text("listener test"),
            (history, 0u32.to_ne_bytes().to_vec()),
        ]);
        pause();
        let history_ok = copy_twice(&[
            text("listener test"),
            (history, 1u32.to_ne_bytes().to_vec()),
        ]);
        pause();
        listener.own_copy_started(1);
        let own = copy_twice(&[text("listener test")]);
        listener.own_copy_finished(1);

        drop(listener);
        write(owner.0, &original);
        assert_eq!(plain, Some(DoubleCopy::Text("listener test".into())));
        assert_eq!(
            excluded, None,
            "ExcludeClipboardContentFromMonitorProcessing"
        );
        assert_eq!(no_history, None, "CanIncludeInClipboardHistory = 0");
        assert_eq!(
            history_ok,
            Some(DoubleCopy::Text("listener test".into())),
            "CanIncludeInClipboardHistory = 1"
        );
        assert_eq!(own, None, "own capture");
    }
}
