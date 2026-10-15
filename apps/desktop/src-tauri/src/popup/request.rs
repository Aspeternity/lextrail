//! One lookup, from hotkey to a focused result (ARCHITECTURE.md 9.3), as a pure state
//! machine. The popup window lifecycle ([`super::lifecycle`]) decides whether a window
//! exists and is awake; this decides when it appears, with what, and when it takes focus.
//!
//! - The popup never takes focus while text is captured: the source window keeps it, so
//!   UIA sees the right focus and the clipboard fallback can send Ctrl+C there.
//! - It appears only once the capture is done, or after [`LOADING_DELAY`] with
//!   "capturing...", shown without activation.
//! - When the capture is done it shows the result and takes focus (for Esc and
//!   click-outside). Until that worked, another window coming to the foreground hides it.

use std::time::Duration;

use super::content::Content;

/// How long capture may take before the popup appears with "capturing...".
pub const LOADING_DELAY: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Hotkey pressed; supersedes any earlier request.
    Started {
        id: u64,
    },
    /// The window exists, is positioned and awake (lifecycle `Reveal`).
    WindowReady,
    DelayElapsed {
        id: u64,
    },
    Captured {
        id: u64,
        content: Content,
    },
    /// Result of trying to give the popup focus.
    Activated {
        ok: bool,
    },
    /// Another window came to the foreground.
    ForegroundChanged,
    /// The popup was hidden or destroyed (Esc, click outside, ...).
    Hidden,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    SetContent(Content),
    /// Show the window without activating it.
    ShowInactive,
    /// Give the window focus, then report [`Event::Activated`].
    Activate,
    /// Start or stop reporting [`Event::ForegroundChanged`].
    WatchForeground(bool),
    /// Hide the popup (through the window lifecycle).
    Hide,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Idle,
    Capturing {
        id: u64,
        window_ready: bool,
        delay_elapsed: bool,
        shown: bool,
    },
    /// Captured before the window was ready (first show, page still loading).
    WaitingForWindow {
        content: Content,
    },
    Activating,
    Shown {
        focused: bool,
    },
}

#[derive(Debug)]
pub struct Request {
    phase: Phase,
}

impl Default for Request {
    fn default() -> Self {
        Self::new()
    }
}

impl Request {
    pub fn new() -> Self {
        Self { phase: Phase::Idle }
    }

    /// The request currently being captured, if any.
    #[cfg(test)]
    pub fn capturing_id(&self) -> Option<u64> {
        match self.phase {
            Phase::Capturing { id, .. } => Some(id),
            _ => None,
        }
    }

    pub fn handle(&mut self, event: Event) -> Vec<Action> {
        use Action as A;
        match (&mut self.phase, event) {
            (_, Event::Started { id }) => {
                self.phase = Phase::Capturing {
                    id,
                    window_ready: false,
                    delay_elapsed: false,
                    shown: false,
                };
                vec![A::SetContent(Content::Empty), A::WatchForeground(true)]
            }

            (
                Phase::Capturing {
                    window_ready,
                    delay_elapsed,
                    shown,
                    ..
                },
                Event::WindowReady,
            ) => {
                *window_ready = true;
                show_loading_if_due(*window_ready, *delay_elapsed, shown)
            }
            (
                Phase::Capturing {
                    id,
                    window_ready,
                    delay_elapsed,
                    shown,
                },
                Event::DelayElapsed { id: fired },
            ) if *id == fired => {
                *delay_elapsed = true;
                show_loading_if_due(*window_ready, *delay_elapsed, shown)
            }
            (
                Phase::Capturing {
                    id,
                    window_ready,
                    shown,
                    ..
                },
                Event::Captured {
                    id: captured,
                    content,
                },
            ) if *id == captured => {
                if !*window_ready {
                    self.phase = Phase::WaitingForWindow { content };
                    return vec![];
                }
                let mut actions = vec![A::SetContent(content)];
                if !*shown {
                    actions.push(A::ShowInactive);
                }
                actions.push(A::Activate);
                self.phase = Phase::Activating;
                actions
            }
            (Phase::WaitingForWindow { content }, Event::WindowReady) => {
                let content = std::mem::replace(content, Content::Empty);
                self.phase = Phase::Activating;
                vec![A::SetContent(content), A::ShowInactive, A::Activate]
            }

            (Phase::Activating, Event::Activated { ok }) => {
                self.phase = Phase::Shown { focused: ok };
                // Focused: click-outside hides it from now on. Not focused (foreground
                // lock): keep watching, so switching windows still hides it.
                if ok {
                    vec![A::WatchForeground(false)]
                } else {
                    vec![]
                }
            }

            (
                Phase::Capturing { .. }
                | Phase::WaitingForWindow { .. }
                | Phase::Activating
                | Phase::Shown { focused: false },
                Event::ForegroundChanged,
            ) => {
                self.phase = Phase::Idle;
                vec![A::WatchForeground(false), A::Hide]
            }

            (_, Event::Hidden) => {
                self.phase = Phase::Idle;
                vec![A::WatchForeground(false), A::SetContent(Content::Empty)]
            }

            // Stale timers and captures of superseded requests, repeated readiness, ...
            _ => vec![],
        }
    }
}

fn show_loading_if_due(window_ready: bool, delay_elapsed: bool, shown: &mut bool) -> Vec<Action> {
    if window_ready && delay_elapsed && !*shown {
        *shown = true;
        vec![Action::SetContent(Content::Loading), Action::ShowInactive]
    } else {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Action as A;

    fn result() -> Content {
        Content::Unavailable {
            reason: "no_selection",
        }
    }

    fn started(id: u64) -> Request {
        let mut r = Request::new();
        assert_eq!(
            r.handle(Event::Started { id }),
            [A::SetContent(Content::Empty), A::WatchForeground(true)]
        );
        r
    }

    #[test]
    fn fast_capture_shows_result_directly_and_takes_focus() {
        let mut r = started(1);
        assert_eq!(r.handle(Event::WindowReady), []);
        assert_eq!(
            r.handle(Event::Captured {
                id: 1,
                content: result()
            }),
            [A::SetContent(result()), A::ShowInactive, A::Activate]
        );
        // The delay timer fires late: nothing happens.
        assert_eq!(r.handle(Event::DelayElapsed { id: 1 }), []);
        assert_eq!(
            r.handle(Event::Activated { ok: true }),
            [A::WatchForeground(false)]
        );
        // Focused: foreground changes are handled by focus loss instead.
        assert_eq!(r.handle(Event::ForegroundChanged), []);
    }

    #[test]
    fn slow_capture_shows_loading_after_the_delay_without_focus() {
        let mut r = started(1);
        r.handle(Event::WindowReady);
        assert_eq!(
            r.handle(Event::DelayElapsed { id: 1 }),
            [A::SetContent(Content::Loading), A::ShowInactive]
        );
        assert_eq!(
            r.handle(Event::Captured {
                id: 1,
                content: result()
            }),
            [A::SetContent(result()), A::Activate]
        );
    }

    #[test]
    fn delay_before_window_ready_shows_loading_once_ready() {
        let mut r = started(1);
        assert_eq!(r.handle(Event::DelayElapsed { id: 1 }), []);
        assert_eq!(
            r.handle(Event::WindowReady),
            [A::SetContent(Content::Loading), A::ShowInactive]
        );
        assert_eq!(r.handle(Event::WindowReady), []);
    }

    #[test]
    fn capture_before_window_ready_waits_for_the_window() {
        // Cold start: the page loads after the capture is done.
        let mut r = started(1);
        assert_eq!(
            r.handle(Event::Captured {
                id: 1,
                content: result()
            }),
            []
        );
        assert_eq!(r.handle(Event::DelayElapsed { id: 1 }), []);
        assert_eq!(
            r.handle(Event::WindowReady),
            [A::SetContent(result()), A::ShowInactive, A::Activate]
        );
    }

    #[test]
    fn foreground_change_during_capture_hides_and_ignores_the_result() {
        let mut r = started(1);
        r.handle(Event::WindowReady);
        r.handle(Event::DelayElapsed { id: 1 });
        assert_eq!(
            r.handle(Event::ForegroundChanged),
            [A::WatchForeground(false), A::Hide]
        );
        assert_eq!(
            r.handle(Event::Captured {
                id: 1,
                content: result()
            }),
            []
        );
    }

    #[test]
    fn failed_activation_keeps_watching_the_foreground() {
        let mut r = started(1);
        r.handle(Event::WindowReady);
        r.handle(Event::Captured {
            id: 1,
            content: result(),
        });
        assert_eq!(r.handle(Event::Activated { ok: false }), []);
        assert_eq!(
            r.handle(Event::ForegroundChanged),
            [A::WatchForeground(false), A::Hide]
        );
    }

    #[test]
    fn new_request_supersedes_the_old_one() {
        let mut r = started(1);
        r.handle(Event::WindowReady);
        r.handle(Event::Started { id: 2 });
        assert_eq!(r.capturing_id(), Some(2));
        assert_eq!(
            r.handle(Event::Captured {
                id: 1,
                content: result()
            }),
            []
        );
        assert_eq!(r.handle(Event::DelayElapsed { id: 1 }), []);
        r.handle(Event::WindowReady);
        assert_eq!(
            r.handle(Event::Captured {
                id: 2,
                content: result()
            }),
            [A::SetContent(result()), A::ShowInactive, A::Activate]
        );
    }

    #[test]
    fn hidden_clears_content_and_stops_watching() {
        let mut r = started(1);
        r.handle(Event::WindowReady);
        r.handle(Event::Captured {
            id: 1,
            content: result(),
        });
        r.handle(Event::Activated { ok: true });
        assert_eq!(
            r.handle(Event::Hidden),
            [A::WatchForeground(false), A::SetContent(Content::Empty)]
        );
        assert_eq!(r.capturing_id(), None);
    }
}
