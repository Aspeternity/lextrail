//! Bringing the popup to the foreground (D042).
//!
//! Windows lets a process take the foreground when it received the last input; the hotkey
//! press normally counts. When it does not (input arrived meanwhile, e.g. a mouse move
//! during a slow capture), the input queue of the current foreground thread is attached
//! briefly, which lets the call through. Tauri's own fallback (`set_focus`) is not used:
//! it simulates an Alt key press, which reaches the window the text came from and can
//! open its menu bar.

/// How the popup got (or did not get) the foreground.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// `SetForegroundWindow` succeeded on its own.
    Direct,
    /// It succeeded after attaching to the foreground thread's input queue.
    Attached,
    /// Windows refused; the popup stays visible without focus.
    Refused,
    /// The foreground window is not responding; attaching to it could hang us.
    ForegroundHung,
}

#[cfg(windows)]
pub fn bring_to_front(hwnd: windows::Win32::Foundation::HWND) -> Activation {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsHungAppWindow, SetForegroundWindow,
    };

    // Debug builds only: LEXTRAIL_DEBUG_FORCE_ATTACH=1 skips the plain call, so tests can
    // check the fallback (it is what runs after a click outside closed the last popup).
    #[cfg(debug_assertions)]
    let force_attach = std::env::var_os("LEXTRAIL_DEBUG_FORCE_ATTACH").is_some();
    #[cfg(not(debug_assertions))]
    let force_attach = false;

    // SAFETY: plain Win32 calls; the attachment is always undone below.
    unsafe {
        if !force_attach && SetForegroundWindow(hwnd).as_bool() {
            return Activation::Direct;
        }
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() {
            return Activation::Refused;
        }
        if IsHungAppWindow(foreground).as_bool() {
            return Activation::ForegroundHung;
        }
        let foreground_thread = GetWindowThreadProcessId(foreground, None);
        let own_thread = GetCurrentThreadId();
        if foreground_thread == 0 || foreground_thread == own_thread {
            return Activation::Refused;
        }
        let attached = AttachThreadInput(own_thread, foreground_thread, true).as_bool();
        let ok = SetForegroundWindow(hwnd).as_bool();
        if attached {
            let _ = AttachThreadInput(own_thread, foreground_thread, false);
        }
        if ok && attached {
            Activation::Attached
        } else if ok {
            Activation::Direct
        } else {
            Activation::Refused
        }
    }
}
