//! Popup lifecycle (D009) as a pure state machine. The window code feeds it events
//! and carries out the returned actions, so every transition is unit-testable.
//!
//! ```text
//! Absent ─show─▶ Creating ─page ready─▶ Visible ─hide─▶ Hidden ─timer─▶ Suspending ─▶ Suspended
//!                                           ▲                │                             │
//!                                           └────────show────┴─────────────show────────────┘
//! ```
//! Destroyed (back to `Absent`) only on low memory or in memory-saver mode.

use std::time::Duration;

use lextrail_config::PopupMode;

/// How long a hidden popup waits before it is suspended. A quick follow-up lookup
/// then skips the resume step entirely.
pub const SUSPEND_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Absent,
    /// Window created, waiting for the page to report it is ready before it is shown.
    Creating,
    Visible,
    /// Hidden; suspended when the timer with this generation fires.
    Hidden {
        generation: u64,
    },
    Suspending,
    Suspended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    ShowRequested,
    /// The page has loaded and subscribed to content events (`popup_ready`).
    PageReady,
    /// Esc, or focus moved elsewhere.
    HideRequested,
    SuspendTimerFired {
        generation: u64,
    },
    SuspendFinished {
        suspended: bool,
    },
    LowMemory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Create,
    /// Move next to the cursor and show; `resume` first wakes a suspended WebView.
    Reveal {
        resume: bool,
    },
    Hide,
    ScheduleSuspend {
        generation: u64,
        after: Duration,
    },
    Suspend,
    Destroy,
}

#[derive(Debug)]
pub struct Lifecycle {
    state: State,
    next_generation: u64,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl Lifecycle {
    pub fn new() -> Self {
        Self {
            state: State::Absent,
            next_generation: 0,
        }
    }

    #[cfg(test)]
    pub fn state(&self) -> State {
        self.state
    }

    /// Applies `event` and returns the actions to perform, in order.
    pub fn handle(&mut self, event: Event, mode: PopupMode) -> Vec<Action> {
        use Action as A;
        use Event as E;
        use State as S;

        let (next, actions) = match (self.state, event) {
            (S::Absent, E::ShowRequested) => (S::Creating, vec![A::Create]),
            // Already on its way; it is shown when the page has loaded.
            (S::Creating, E::ShowRequested) => (S::Creating, vec![]),
            (S::Creating, E::PageReady) => (S::Visible, vec![A::Reveal { resume: false }]),
            (S::Visible | S::Hidden { .. }, E::ShowRequested) => {
                (S::Visible, vec![A::Reveal { resume: false }])
            }
            // Resuming while a suspend is still in flight is allowed by WebView2.
            (S::Suspending | S::Suspended, E::ShowRequested) => {
                (S::Visible, vec![A::Reveal { resume: true }])
            }

            (S::Visible, E::HideRequested) => match mode {
                PopupMode::MemorySaver => (S::Absent, vec![A::Destroy]),
                PopupMode::Responsive => {
                    let generation = self.bump_generation();
                    (
                        S::Hidden { generation },
                        vec![
                            A::Hide,
                            A::ScheduleSuspend {
                                generation,
                                after: SUSPEND_DELAY,
                            },
                        ],
                    )
                }
            },
            // Esc during loading: the user no longer wants it.
            (S::Creating, E::HideRequested) => (S::Absent, vec![A::Destroy]),

            (S::Hidden { generation }, E::SuspendTimerFired { generation: fired })
                if generation == fired =>
            {
                (S::Suspending, vec![A::Suspend])
            }

            (S::Suspending, E::SuspendFinished { suspended: true }) => (S::Suspended, vec![]),
            // Stay hidden and unsuspended; the next hide schedules another attempt.
            (S::Suspending, E::SuspendFinished { suspended: false }) => {
                let generation = self.bump_generation();
                (S::Hidden { generation }, vec![])
            }

            // Only free memory the user is not looking at.
            (S::Hidden { .. } | S::Suspending | S::Suspended, E::LowMemory) => {
                (S::Absent, vec![A::Destroy])
            }

            // Everything else is stale or irrelevant (old timers, focus changes while
            // loading, low memory while visible, ...).
            (state, _) => (state, vec![]),
        };
        self.state = next;
        actions
    }

    fn bump_generation(&mut self) -> u64 {
        self.next_generation += 1;
        self.next_generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: PopupMode = PopupMode::Responsive;
    const M: PopupMode = PopupMode::MemorySaver;

    /// Drives a fresh lifecycle to `Visible`.
    fn visible() -> Lifecycle {
        let mut lc = Lifecycle::new();
        lc.handle(Event::ShowRequested, R);
        lc.handle(Event::PageReady, R);
        assert_eq!(lc.state(), State::Visible);
        lc
    }

    /// Hides a visible popup and returns the suspend timer generation.
    fn hide(lc: &mut Lifecycle) -> u64 {
        let actions = lc.handle(Event::HideRequested, R);
        match actions.as_slice() {
            [
                Action::Hide,
                Action::ScheduleSuspend {
                    generation,
                    after: SUSPEND_DELAY,
                },
            ] => *generation,
            other => panic!("unexpected actions {other:?}"),
        }
    }

    fn suspended() -> Lifecycle {
        let mut lc = visible();
        let generation = hide(&mut lc);
        lc.handle(Event::SuspendTimerFired { generation }, R);
        lc.handle(Event::SuspendFinished { suspended: true }, R);
        assert_eq!(lc.state(), State::Suspended);
        lc
    }

    #[test]
    fn first_show_creates_then_reveals_after_load() {
        let mut lc = Lifecycle::new();
        assert_eq!(lc.handle(Event::ShowRequested, R), vec![Action::Create]);
        assert_eq!(lc.state(), State::Creating);
        assert_eq!(
            lc.handle(Event::PageReady, R),
            vec![Action::Reveal { resume: false }]
        );
        assert_eq!(lc.state(), State::Visible);
    }

    #[test]
    fn repeated_show_while_creating_does_not_create_twice() {
        let mut lc = Lifecycle::new();
        lc.handle(Event::ShowRequested, R);
        assert_eq!(lc.handle(Event::ShowRequested, R), vec![]);
        assert_eq!(lc.state(), State::Creating);
    }

    #[test]
    fn focus_loss_while_creating_is_ignored() {
        // WebView2 bounces focus while it initialises a hidden window.
        let mut lc = Lifecycle::new();
        lc.handle(Event::ShowRequested, R);
        // HideRequested is only sent for focus loss when the window is visible, but a
        // stray low-memory or timer event must not disturb creation either.
        assert_eq!(lc.handle(Event::LowMemory, R), vec![]);
        assert_eq!(
            lc.handle(Event::SuspendTimerFired { generation: 0 }, R),
            vec![]
        );
        assert_eq!(lc.state(), State::Creating);
    }

    #[test]
    fn esc_while_creating_destroys() {
        let mut lc = Lifecycle::new();
        lc.handle(Event::ShowRequested, R);
        assert_eq!(lc.handle(Event::HideRequested, R), vec![Action::Destroy]);
        assert_eq!(lc.state(), State::Absent);
    }

    #[test]
    fn hide_schedules_suspend_then_suspends() {
        let mut lc = visible();
        let generation = hide(&mut lc);
        assert_eq!(lc.state(), State::Hidden { generation });
        assert_eq!(
            lc.handle(Event::SuspendTimerFired { generation }, R),
            vec![Action::Suspend]
        );
        assert_eq!(lc.state(), State::Suspending);
        assert_eq!(
            lc.handle(Event::SuspendFinished { suspended: true }, R),
            vec![]
        );
        assert_eq!(lc.state(), State::Suspended);
    }

    #[test]
    fn show_from_hidden_reveals_without_resume() {
        let mut lc = visible();
        hide(&mut lc);
        assert_eq!(
            lc.handle(Event::ShowRequested, R),
            vec![Action::Reveal { resume: false }]
        );
        assert_eq!(lc.state(), State::Visible);
    }

    #[test]
    fn show_from_suspended_resumes() {
        let mut lc = suspended();
        assert_eq!(
            lc.handle(Event::ShowRequested, R),
            vec![Action::Reveal { resume: true }]
        );
        assert_eq!(lc.state(), State::Visible);
    }

    #[test]
    fn show_while_suspending_resumes() {
        let mut lc = visible();
        let generation = hide(&mut lc);
        lc.handle(Event::SuspendTimerFired { generation }, R);
        assert_eq!(
            lc.handle(Event::ShowRequested, R),
            vec![Action::Reveal { resume: true }]
        );
        // The late completion callback must not flip a visible popup to suspended.
        assert_eq!(
            lc.handle(Event::SuspendFinished { suspended: true }, R),
            vec![]
        );
        assert_eq!(lc.state(), State::Visible);
    }

    #[test]
    fn stale_timer_after_reshow_is_ignored() {
        let mut lc = visible();
        let old = hide(&mut lc);
        lc.handle(Event::ShowRequested, R);
        let new = hide(&mut lc);
        assert_ne!(old, new);
        assert_eq!(
            lc.handle(Event::SuspendTimerFired { generation: old }, R),
            vec![]
        );
        assert_eq!(lc.state(), State::Hidden { generation: new });
        assert_eq!(
            lc.handle(Event::SuspendTimerFired { generation: new }, R),
            vec![Action::Suspend]
        );
    }

    #[test]
    fn timer_while_visible_is_ignored() {
        let mut lc = visible();
        let generation = hide(&mut lc);
        lc.handle(Event::ShowRequested, R);
        assert_eq!(
            lc.handle(Event::SuspendTimerFired { generation }, R),
            vec![]
        );
        assert_eq!(lc.state(), State::Visible);
    }

    #[test]
    fn failed_suspend_stays_hidden_and_ignores_old_timer() {
        let mut lc = visible();
        let generation = hide(&mut lc);
        lc.handle(Event::SuspendTimerFired { generation }, R);
        assert_eq!(
            lc.handle(Event::SuspendFinished { suspended: false }, R),
            vec![]
        );
        let State::Hidden { generation: now } = lc.state() else {
            panic!("expected hidden, got {:?}", lc.state());
        };
        assert_ne!(now, generation);
        assert_eq!(
            lc.handle(Event::SuspendTimerFired { generation }, R),
            vec![]
        );
        // Showing still works.
        assert_eq!(
            lc.handle(Event::ShowRequested, R),
            vec![Action::Reveal { resume: false }]
        );
    }

    #[test]
    fn hide_twice_is_harmless() {
        let mut lc = visible();
        hide(&mut lc);
        assert_eq!(lc.handle(Event::HideRequested, R), vec![]);
    }

    #[test]
    fn low_memory_destroys_when_not_visible() {
        for mut lc in [
            {
                let mut lc = visible();
                hide(&mut lc);
                lc
            },
            suspended(),
        ] {
            assert_eq!(lc.handle(Event::LowMemory, R), vec![Action::Destroy]);
            assert_eq!(lc.state(), State::Absent);
        }
    }

    #[test]
    fn low_memory_while_suspending_destroys() {
        let mut lc = visible();
        let generation = hide(&mut lc);
        lc.handle(Event::SuspendTimerFired { generation }, R);
        assert_eq!(lc.handle(Event::LowMemory, R), vec![Action::Destroy]);
        assert_eq!(
            lc.handle(Event::SuspendFinished { suspended: true }, R),
            vec![]
        );
        assert_eq!(lc.state(), State::Absent);
    }

    #[test]
    fn low_memory_while_visible_is_ignored() {
        let mut lc = visible();
        assert_eq!(lc.handle(Event::LowMemory, R), vec![]);
        assert_eq!(lc.state(), State::Visible);
    }

    #[test]
    fn show_after_destroy_creates_again() {
        let mut lc = suspended();
        lc.handle(Event::LowMemory, R);
        assert_eq!(lc.handle(Event::ShowRequested, R), vec![Action::Create]);
    }

    #[test]
    fn memory_saver_destroys_on_hide() {
        let mut lc = visible();
        assert_eq!(lc.handle(Event::HideRequested, M), vec![Action::Destroy]);
        assert_eq!(lc.state(), State::Absent);
        assert_eq!(lc.handle(Event::ShowRequested, M), vec![Action::Create]);
    }

    #[test]
    fn suspend_delay_is_a_few_seconds() {
        // D009: "隐藏（几秒）→ 挂起".
        assert!((1..=10).contains(&SUSPEND_DELAY.as_secs()));
    }

    #[test]
    fn page_ready_outside_creation_is_ignored() {
        // E.g. the WebView reloaded the page after a crash: the window state stays.
        let mut lc = visible();
        assert_eq!(lc.handle(Event::PageReady, R), vec![]);
        assert_eq!(lc.state(), State::Visible);

        let mut lc = suspended();
        assert_eq!(lc.handle(Event::PageReady, R), vec![]);
        assert_eq!(lc.state(), State::Suspended);

        let mut lc = Lifecycle::new();
        assert_eq!(lc.handle(Event::PageReady, R), vec![]);
        assert_eq!(lc.state(), State::Absent);
    }

    #[test]
    fn recreated_window_waits_for_page_ready_again() {
        let mut lc = visible();
        lc.handle(Event::HideRequested, M); // memory saver: destroyed
        assert_eq!(lc.state(), State::Absent);
        assert_eq!(lc.handle(Event::ShowRequested, M), vec![Action::Create]);
        // Nothing is revealed until the new page reports ready.
        assert_eq!(lc.handle(Event::ShowRequested, M), vec![]);
        assert_eq!(
            lc.handle(Event::PageReady, M),
            vec![Action::Reveal { resume: false }]
        );
    }
}
