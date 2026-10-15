//! Reports when another application's window comes to the foreground, while the popup
//! is visible but not focused (capture in progress, or activation was refused).
//!
//! Uses an out-of-context `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)`, which delivers
//! events through the message loop of the thread that installed it, so install and
//! remove it on the main (UI) thread.

#[cfg(windows)]
mod imp {
    use std::cell::Cell;
    use std::sync::OnceLock;

    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
    use windows::Win32::UI::WindowsAndMessaging::{
        EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    };

    type Callback = Box<dyn Fn(isize) + Send + Sync>;

    static CALLBACK: OnceLock<Callback> = OnceLock::new();

    thread_local! {
        static HOOK: Cell<Option<HWINEVENTHOOK>> = const { Cell::new(None) };
    }

    /// Sets the function called with the new foreground window. Call once at startup.
    pub fn init(callback: impl Fn(isize) + Send + Sync + 'static) {
        let _ = CALLBACK.set(Box::new(callback));
    }

    /// Starts or stops watching. Must run on the main thread.
    pub fn set_watching(on: bool) {
        HOOK.with(|hook| match (on, hook.get()) {
            (true, None) => {
                // SAFETY: out-of-context hook with a 'static callback; removed below.
                let handle = unsafe {
                    SetWinEventHook(
                        EVENT_SYSTEM_FOREGROUND,
                        EVENT_SYSTEM_FOREGROUND,
                        None,
                        Some(on_event),
                        0,
                        0,
                        // Our own popup taking focus is not a reason to hide it.
                        WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                    )
                };
                if handle.is_invalid() {
                    eprintln!("popup: SetWinEventHook failed");
                } else {
                    hook.set(Some(handle));
                }
            }
            (false, Some(handle)) => {
                // SAFETY: the handle came from SetWinEventHook on this thread.
                let _ = unsafe { UnhookWinEvent(handle) };
                hook.set(None);
            }
            _ => {}
        });
    }

    unsafe extern "system" fn on_event(
        _hook: HWINEVENTHOOK,
        _event: u32,
        hwnd: HWND,
        _id_object: i32,
        _id_child: i32,
        _thread: u32,
        _time: u32,
    ) {
        if let Some(callback) = CALLBACK.get() {
            callback(hwnd.0 as isize);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn init(_callback: impl Fn(isize) + Send + Sync + 'static) {}
    pub fn set_watching(_on: bool) {}
}

pub use imp::{init, set_watching};
