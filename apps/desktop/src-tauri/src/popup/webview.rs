//! WebView2 suspend / resume for the hidden popup (D009). `TrySuspend` and
//! `MemoryUsageTargetLevel` must not be mixed; only the former is used.
//!
//! **Never call back into the app from a `with_webview` closure.** Called on the main
//! thread, Tauri runs the closure synchronously while it still holds the webview's
//! `window_id` lock, and most webview calls (`emit_to`, `eval`, ...) take that lock again:
//! the main thread deadlocks on itself (R4-P1). The callbacks below therefore run on a
//! new thread, after the closure has returned.

use tauri::{Runtime, WebviewWindow};

use super::spawn;

/// Suspends the popup's WebView and reports whether it worked. The WebView must be
/// invisible, so its controller visibility is turned off first. `done` runs on its own
/// thread.
#[cfg(windows)]
pub fn suspend<R: Runtime>(
    window: &WebviewWindow<R>,
    done: impl FnOnce(bool) + Send + 'static,
) -> tauri::Result<()> {
    // A failure is reported inside the closure; the completion handler may run later.
    // Either way `done` must not run there (see the module docs).
    let done = move |ok: bool| spawn("popup-suspended", move || done(ok));
    window.with_webview(move |webview| {
        // SAFETY: called on the UI thread that owns the WebView2 controller.
        if let Err(e) = unsafe { windows_impl::try_suspend(&webview.controller(), done) } {
            eprintln!("TrySuspend failed: {e}");
        }
    })
}

/// Resumes a suspended WebView and makes it visible again, then runs `then` on its own
/// thread, so the page is live before the window appears.
#[cfg(windows)]
pub fn resume_then<R: Runtime>(
    window: &WebviewWindow<R>,
    then: impl FnOnce() + Send + 'static,
) -> tauri::Result<()> {
    window.with_webview(move |webview| {
        // SAFETY: called on the UI thread that owns the WebView2 controller.
        if let Err(e) = unsafe { windows_impl::resume(&webview.controller()) } {
            eprintln!("Resume failed: {e}");
        }
        debug_resume_delay();
        // Not `then()` here: it shows the window and sends content (see the module docs).
        spawn("popup-resumed", then);
    })
}

/// Debug builds only: `LEXTRAIL_DEBUG_RESUME_DELAY_MS` makes `Resume` slow, so that the
/// capture result arrives before the resumed window is ready (R4-P1).
#[cfg(windows)]
fn debug_resume_delay() {
    #[cfg(debug_assertions)]
    if let Some(ms) = std::env::var("LEXTRAIL_DEBUG_RESUME_DELAY_MS")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        eprintln!("popup: resume delayed by {ms} ms (LEXTRAIL_DEBUG_RESUME_DELAY_MS)");
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

#[cfg(not(windows))]
pub fn suspend<R: Runtime>(
    _window: &WebviewWindow<R>,
    done: impl FnOnce(bool) + Send + 'static,
) -> tauri::Result<()> {
    // Not implemented on this platform yet: report "not suspended", from its own thread
    // like on Windows.
    spawn("popup-suspended", move || done(false));
    Ok(())
}

#[cfg(not(windows))]
pub fn resume_then<R: Runtime>(
    _window: &WebviewWindow<R>,
    then: impl FnOnce() + Send + 'static,
) -> tauri::Result<()> {
    spawn("popup-resumed", then);
    Ok(())
}

#[cfg(windows)]
mod windows_impl {
    use std::cell::RefCell;
    use std::rc::Rc;

    use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2_3, ICoreWebView2Controller};
    use webview2_com::TrySuspendCompletedHandler;
    use windows::core::{Interface, Result};

    /// Calls the wrapped callback at most once, from whichever path finishes first.
    struct Once<F>(Rc<RefCell<Option<F>>>);

    impl<F: FnOnce(bool)> Once<F> {
        fn report(&self, ok: bool) {
            if let Some(done) = self.0.borrow_mut().take() {
                done(ok);
            }
        }
    }

    /// # Safety
    /// Must run on the thread that owns `controller`.
    pub unsafe fn try_suspend(
        controller: &ICoreWebView2Controller,
        done: impl FnOnce(bool) + 'static,
    ) -> Result<()> {
        let done = Rc::new(RefCell::new(Some(done)));
        let from_handler = Once(Rc::clone(&done));
        let result = unsafe {
            controller.SetIsVisible(false).and_then(|()| {
                let core: ICoreWebView2_3 = controller.CoreWebView2()?.cast()?;
                let handler =
                    TrySuspendCompletedHandler::create(Box::new(move |result, suspended| {
                        from_handler.report(result.is_ok() && suspended);
                        Ok(())
                    }));
                core.TrySuspend(&handler)
            })
        };
        // If TrySuspend never started, its handler will not run: report failure here
        // so the lifecycle does not stay in "suspending".
        if result.is_err() {
            Once(done).report(false);
        }
        result
    }

    /// # Safety
    /// Must run on the thread that owns `controller`.
    pub unsafe fn resume(controller: &ICoreWebView2Controller) -> Result<()> {
        unsafe {
            let core: ICoreWebView2_3 = controller.CoreWebView2()?.cast()?;
            core.Resume()?;
            controller.SetIsVisible(true)
        }
    }
}
