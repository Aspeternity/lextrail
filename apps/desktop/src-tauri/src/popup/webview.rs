//! WebView2 suspend / resume for the hidden popup (D009). `TrySuspend` and
//! `MemoryUsageTargetLevel` must not be mixed; only the former is used.

use tauri::{Runtime, WebviewWindow};

/// Suspends the popup's WebView and reports whether it worked. The WebView must be
/// invisible, so its controller visibility is turned off first.
#[cfg(windows)]
pub fn suspend<R: Runtime>(
    window: &WebviewWindow<R>,
    done: impl FnOnce(bool) + Send + 'static,
) -> tauri::Result<()> {
    window.with_webview(move |webview| {
        // SAFETY: called on the UI thread that owns the WebView2 controller.
        if let Err(e) = unsafe { windows_impl::try_suspend(&webview.controller(), done) } {
            eprintln!("TrySuspend failed: {e}");
        }
    })
}

/// Resumes a suspended WebView and makes it visible again, then runs `then` on the UI
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
        then();
    })
}

#[cfg(not(windows))]
pub fn suspend<R: Runtime>(
    _window: &WebviewWindow<R>,
    done: impl FnOnce(bool) + Send + 'static,
) -> tauri::Result<()> {
    // Not implemented on this platform yet: report "not suspended".
    done(false);
    Ok(())
}

#[cfg(not(windows))]
pub fn resume_then<R: Runtime>(
    _window: &WebviewWindow<R>,
    then: impl FnOnce() + Send + 'static,
) -> tauri::Result<()> {
    then();
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
