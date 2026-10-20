//! One lookup, from hotkey to a focused result (ARCHITECTURE.md 9.3), as a pure state
//! machine. The popup window lifecycle ([`super::lifecycle`]) decides whether a window
//! exists and is awake; this decides when it appears, with what, and when it takes focus.
//!
//! - The popup never takes focus while text is captured: the source window keeps it, so
//!   UIA sees the right focus and the clipboard fallback can send Ctrl+C there.
//! - It appears only once the capture is done, or after [`LOADING_DELAY`] with
//!   "capturing...", shown without activation.
//! - Time the user spends holding the hotkey (the clipboard fallback waits for its keys
//!   to be released) does not count towards [`LOADING_DELAY`]: the delay starts again
//!   when the keys are released, so a normal key press shows no "capturing..." flash
//!   (R3-P4). Held for [`KEYS_HINT_DELAY`], the popup asks for the keys to be released.
//! - When the capture is done it shows the result and takes focus (for Esc and
//!   click-outside). Foreground changes while taking focus are ignored (they are ours).
//! - If Windows refuses the focus, the result stays visible without it: another window
//!   coming to the foreground hides it, and so does the page's close button.

use std::time::Duration;

use super::content::Content;

/// How long capture may take before the popup appears with "capturing...".
pub const LOADING_DELAY: Duration = Duration::from_millis(100);
/// How long the hotkey may be held before the popup asks for it to be released.
pub const KEYS_HINT_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Hotkey pressed; supersedes any earlier request.
    Started {
        id: u64,
    },
    /// The window exists, is positioned and awake (lifecycle `Reveal`).
    WindowReady,
    /// A timer started by [`Action::StartTimer`] fired.
    TimerFired {
        id: u64,
        timer: u32,
    },
    /// The capture is waiting for the hotkey's keys to be released.
    WaitingForKeys {
        id: u64,
    },
    /// They were released.
    KeysReleased {
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
    /// Report [`Event::TimerFired`] with this `id` and `timer` after `after`. Only the
    /// latest timer of a request counts; earlier ones are ignored when they fire.
    StartTimer {
        id: u64,
        timer: u32,
        after: Duration,
    },
}

/// A request whose capture is still running.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Capturing {
    id: u64,
    window_ready: bool,
    /// The current timer: its [`Event::TimerFired`] counts, older ones do not.
    timer: u32,
    /// The current timer has fired.
    due: bool,
    /// Waiting for the hotkey's keys: the timer is the keys hint, not "capturing...".
    keys_held: bool,
    shown: bool,
    hint_shown: bool,
}

impl Capturing {
    fn new(id: u64) -> Self {
        Self {
            id,
            window_ready: false,
            timer: 0,
            due: false,
            keys_held: false,
            shown: false,
            hint_shown: false,
        }
    }

    /// Replaces the current timer.
    fn restart_timer(&mut self, after: Duration) -> Vec<Action> {
        self.timer += 1;
        self.due = false;
        vec![Action::StartTimer {
            id: self.id,
            timer: self.timer,
            after,
        }]
    }

    /// Shows "capturing..." or the keys hint once the window is ready and its timer fired.
    fn show_if_due(&mut self) -> Vec<Action> {
        if !(self.window_ready && self.due) {
            return vec![];
        }
        let content = if self.keys_held {
            if self.hint_shown {
                return vec![];
            }
            self.hint_shown = true;
            Content::ReleaseKeys
        } else if self.shown {
            // Already showing "capturing..." or the hint (released, copy still running).
            return vec![];
        } else {
            Content::Loading
        };
        let mut actions = vec![Action::SetContent(content)];
        if !self.shown {
            self.shown = true;
            actions.push(Action::ShowInactive);
        }
        actions
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Idle,
    Capturing(Capturing),
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
        match &self.phase {
            Phase::Capturing(c) => Some(c.id),
            _ => None,
        }
    }

    pub fn handle(&mut self, event: Event) -> Vec<Action> {
        use Action as A;
        match (&mut self.phase, event) {
            (_, Event::Started { id }) => {
                let mut capturing = Capturing::new(id);
                let mut actions = vec![A::SetContent(Content::Empty), A::WatchForeground(true)];
                actions.extend(capturing.restart_timer(LOADING_DELAY));
                self.phase = Phase::Capturing(capturing);
                actions
            }

            (Phase::Capturing(c), Event::WindowReady) => {
                c.window_ready = true;
                c.show_if_due()
            }
            (Phase::Capturing(c), Event::TimerFired { id, timer })
                if c.id == id && c.timer == timer =>
            {
                c.due = true;
                c.show_if_due()
            }
            (Phase::Capturing(c), Event::WaitingForKeys { id }) if c.id == id => {
                c.keys_held = true;
                c.restart_timer(KEYS_HINT_DELAY)
            }
            (Phase::Capturing(c), Event::KeysReleased { id }) if c.id == id => {
                c.keys_held = false;
                c.restart_timer(LOADING_DELAY)
            }
            (Phase::Capturing(c), Event::Captured { id, content }) if c.id == id => {
                if !c.window_ready {
                    self.phase = Phase::WaitingForWindow { content };
                    return vec![];
                }
                let mut actions = vec![A::SetContent(content)];
                if !c.shown {
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

            // While we are taking the foreground ourselves, foreground changes are ours (or
            // a side effect of trying), not the user switching windows.
            (Phase::Activating, Event::ForegroundChanged) => vec![],
            (
                Phase::Capturing(_)
                | Phase::WaitingForWindow { .. }
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

#[cfg(test)]
mod tests {
    use super::*;
    use Action as A;

    fn result() -> Content {
        Content::Unavailable {
            reason: "no_selection",
        }
    }

    fn timer(id: u64, timer: u32, after: Duration) -> Action {
        A::StartTimer { id, timer, after }
    }

    fn started(id: u64) -> Request {
        let mut r = Request::new();
        assert_eq!(
            r.handle(Event::Started { id }),
            [
                A::SetContent(Content::Empty),
                A::WatchForeground(true),
                timer(id, 1, LOADING_DELAY)
            ]
        );
        r
    }

    fn fired(id: u64, timer: u32) -> Event {
        Event::TimerFired { id, timer }
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
        assert_eq!(r.handle(fired(1, 1)), []);
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
            r.handle(fired(1, 1)),
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
        assert_eq!(r.handle(fired(1, 1)), []);
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
        assert_eq!(r.handle(fired(1, 1)), []);
        assert_eq!(
            r.handle(Event::WindowReady),
            [A::SetContent(result()), A::ShowInactive, A::Activate]
        );
    }

    #[test]
    fn waiting_for_keys_does_not_count_towards_the_loading_delay() {
        // A normal key press in a clipboard-fallback program (R3-P4).
        let mut r = started(1);
        r.handle(Event::WindowReady);
        assert_eq!(
            r.handle(Event::WaitingForKeys { id: 1 }),
            [timer(1, 2, KEYS_HINT_DELAY)]
        );
        // The first delay fires while the keys are still held: ignored.
        assert_eq!(r.handle(fired(1, 1)), []);
        assert_eq!(
            r.handle(Event::KeysReleased { id: 1 }),
            [timer(1, 3, LOADING_DELAY)]
        );
        // The hint timer of the wait fires after the release: ignored too.
        assert_eq!(r.handle(fired(1, 2)), []);
        // The copy is done within the new delay: the result comes up directly.
        assert_eq!(
            r.handle(Event::Captured {
                id: 1,
                content: result()
            }),
            [A::SetContent(result()), A::ShowInactive, A::Activate]
        );
    }

    #[test]
    fn slow_copy_after_the_release_still_shows_loading() {
        let mut r = started(1);
        r.handle(Event::WindowReady);
        r.handle(Event::WaitingForKeys { id: 1 });
        r.handle(Event::KeysReleased { id: 1 });
        assert_eq!(
            r.handle(fired(1, 3)),
            [A::SetContent(Content::Loading), A::ShowInactive]
        );
    }

    #[test]
    fn keys_held_long_shows_the_hint_then_the_result() {
        let mut r = started(1);
        r.handle(Event::WindowReady);
        r.handle(Event::WaitingForKeys { id: 1 });
        assert_eq!(
            r.handle(fired(1, 2)),
            [A::SetContent(Content::ReleaseKeys), A::ShowInactive]
        );
        // Released: the hint stays up (it says what is happening) until the result.
        assert_eq!(
            r.handle(Event::KeysReleased { id: 1 }),
            [timer(1, 3, LOADING_DELAY)]
        );
        assert_eq!(r.handle(fired(1, 3)), []);
        assert_eq!(
            r.handle(Event::Captured {
                id: 1,
                content: result()
            }),
            [A::SetContent(result()), A::Activate]
        );
    }

    #[test]
    fn hint_replaces_loading_shown_before_the_wait() {
        // A slow UIA step showed "capturing..." before the clipboard step started waiting.
        let mut r = started(1);
        r.handle(Event::WindowReady);
        r.handle(fired(1, 1));
        r.handle(Event::WaitingForKeys { id: 1 });
        assert_eq!(r.handle(fired(1, 2)), [A::SetContent(Content::ReleaseKeys)]);
    }

    #[test]
    fn hint_waits_for_the_window() {
        let mut r = started(1);
        r.handle(Event::WaitingForKeys { id: 1 });
        assert_eq!(r.handle(fired(1, 2)), []);
        assert_eq!(
            r.handle(Event::WindowReady),
            [A::SetContent(Content::ReleaseKeys), A::ShowInactive]
        );
    }

    #[test]
    fn progress_of_a_superseded_request_is_ignored() {
        let mut r = started(1);
        r.handle(Event::Started { id: 2 });
        assert_eq!(r.handle(Event::WaitingForKeys { id: 1 }), []);
        assert_eq!(r.handle(Event::KeysReleased { id: 1 }), []);
        assert_eq!(r.handle(fired(1, 1)), []);
    }

    #[test]
    fn foreground_change_during_capture_hides_and_ignores_the_result() {
        let mut r = started(1);
        r.handle(Event::WindowReady);
        r.handle(fired(1, 1));
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
        assert_eq!(r.handle(fired(1, 1)), []);
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

    #[test]
    fn foreground_changes_while_activating_are_ignored() {
        let mut r = started(1);
        r.handle(Event::WindowReady);
        r.handle(Event::Captured {
            id: 1,
            content: result(),
        });
        // E.g. a side effect of attaching to the foreground thread to take the focus.
        assert_eq!(r.handle(Event::ForegroundChanged), []);
        // Refused: the result stays visible without focus.
        assert_eq!(r.handle(Event::Activated { ok: false }), []);
        // From now on the user switching windows hides it.
        assert_eq!(
            r.handle(Event::ForegroundChanged),
            [A::WatchForeground(false), A::Hide]
        );
    }
}
