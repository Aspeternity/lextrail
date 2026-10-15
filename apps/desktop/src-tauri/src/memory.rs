//! System low-memory notifications (D009: destroy the hidden popup when memory is low).

use std::time::Duration;

/// While memory stays low, how often to re-check before waiting for the next episode.
/// The low-memory notification is a level (stays signaled), not an edge.
#[cfg_attr(not(windows), allow(dead_code))]
const RECHECK_WHILE_LOW: Duration = Duration::from_secs(60);

/// Calls `on_low` on a background thread whenever the system reports low memory.
/// The thread just blocks on the OS notification; nothing polls while memory is fine.
#[cfg(windows)]
pub fn watch(on_low: impl Fn() + Send + 'static) {
    use windows::Win32::System::Memory::{
        CreateMemoryResourceNotification, LowMemoryResourceNotification,
    };
    use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};

    // SAFETY: plain Win32 call; the handle lives for the rest of the process.
    let handle = match unsafe { CreateMemoryResourceNotification(LowMemoryResourceNotification) } {
        Ok(handle) => handle,
        Err(e) => {
            eprintln!("low-memory notifications unavailable: {e}");
            return;
        }
    };
    let handle = SendHandle(handle);
    let spawned = std::thread::Builder::new()
        .name("low-memory-watch".into())
        .spawn(move || {
            let handle = handle;
            loop {
                // SAFETY: `handle` is a valid memory resource notification handle.
                unsafe { WaitForSingleObject(handle.0, INFINITE) };
                on_low();
                std::thread::sleep(RECHECK_WHILE_LOW);
            }
        });
    if let Err(e) = spawned {
        eprintln!("cannot start low-memory watcher: {e}");
    }
}

#[cfg(not(windows))]
pub fn watch(_on_low: impl Fn() + Send + 'static) {}

/// A Win32 handle moved to the watcher thread. Waiting on it from another thread is
/// allowed; it is never closed.
#[cfg(windows)]
struct SendHandle(windows::Win32::Foundation::HANDLE);

// SAFETY: kernel object handles may be used from any thread.
#[cfg(windows)]
unsafe impl Send for SendHandle {}
