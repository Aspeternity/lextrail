//! Holding the popup back until its page has measured the content (D046 supplement), so
//! it never shows at the old size and then jumps to the new one.
//!
//! Every content sent to the page gets a number. The page lays it out, measures its
//! natural height and reports it with that number (`popup_measured`). Showing and
//! activating the window wait for the report of the latest content, at most
//! [`MEASURE_WAIT`]; then the window is sized and placed first, and the held actions run.
//! A page that does not report in time gets the default height: the popup is never kept
//! from showing. Pure logic: the window work happens in `popup/mod.rs`.

use std::time::Duration;

use super::request::Action;

/// How long showing waits for the page's height before using the default.
pub const MEASURE_WAIT: Duration = Duration::from_millis(50);

/// What to do once the content's height is known (or given up on).
#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    /// The content's natural height in logical pixels; `None`: use the default.
    pub height: Option<f64>,
    /// Held actions, in order: run them after sizing the window. Empty when only the
    /// content changed (size the window only if it is on screen).
    pub actions: Vec<Action>,
}

#[derive(Debug, Default)]
pub struct Sizing {
    /// The number of the latest content sent to the page.
    sent: u64,
    /// Its height, once reported.
    measured: Option<f64>,
    /// Actions waiting for that height.
    held: Vec<Action>,
    /// The content number a wait timer runs for.
    timer: Option<u64>,
}

impl Sizing {
    pub fn new() -> Self {
        Self::default()
    }

    /// A new content goes to the page. Returns its number, and whether a wait timer must
    /// start for it (actions are already waiting, now for this content).
    pub fn content_sent(&mut self) -> (u64, bool) {
        self.sent += 1;
        self.measured = None;
        (self.sent, self.start_timer())
    }

    /// The number of the latest content sent.
    pub fn current(&self) -> u64 {
        self.sent
    }

    /// The window is to be shown or activated. Runs now (after sizing) when the current
    /// content's height is known; otherwise held, and `Err(true)` when a wait timer must
    /// start.
    pub fn hold(&mut self, action: Action) -> Result<Release, bool> {
        self.held.push(action);
        match self.measured {
            Some(height) => Ok(self.release(Some(height))),
            None => Err(self.start_timer()),
        }
    }

    /// The page measured content `seq`. Reports for older contents are ignored.
    pub fn measured(&mut self, seq: u64, height: f64) -> Option<Release> {
        if seq != self.sent {
            return None;
        }
        self.measured = Some(height);
        Some(self.release(Some(height)))
    }

    /// The wait timer for content `seq` ran out: release with the default height if the
    /// page has still not reported.
    pub fn timed_out(&mut self, seq: u64) -> Option<Release> {
        if self.timer == Some(seq) {
            self.timer = None;
        }
        if seq != self.sent || self.measured.is_some() || self.held.is_empty() {
            return None;
        }
        Some(self.release(None))
    }

    /// The popup was hidden: nothing waits any more.
    pub fn reset(&mut self) {
        self.held.clear();
        self.timer = None;
    }

    fn start_timer(&mut self) -> bool {
        if self.held.is_empty() || self.timer == Some(self.sent) {
            return false;
        }
        self.timer = Some(self.sent);
        true
    }

    fn release(&mut self, height: Option<f64>) -> Release {
        self.timer = None;
        Release {
            height,
            actions: std::mem::take(&mut self.held),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOW: Action = Action::ShowInactive;
    const ACTIVATE: Action = Action::Activate;

    #[test]
    fn showing_waits_for_the_height_then_sizes_first() {
        let mut s = Sizing::new();
        let (seq, timer) = s.content_sent();
        assert!(!timer, "nothing waits yet");
        assert_eq!(s.hold(SHOW), Err(true));
        assert_eq!(s.hold(ACTIVATE), Err(false), "one timer per content");
        assert_eq!(
            s.measured(seq, 180.5),
            Some(Release {
                height: Some(180.5),
                actions: vec![SHOW, ACTIVATE],
            })
        );
        // Nothing left for the timer.
        assert_eq!(s.timed_out(seq), None);
    }

    #[test]
    fn a_page_that_does_not_report_gets_the_default_height() {
        let mut s = Sizing::new();
        let (seq, _) = s.content_sent();
        assert_eq!(s.hold(SHOW), Err(true));
        assert_eq!(
            s.timed_out(seq),
            Some(Release {
                height: None,
                actions: vec![SHOW],
            })
        );
        // A late report still sizes the window (if it is on screen), with nothing held.
        assert_eq!(
            s.measured(seq, 300.0),
            Some(Release {
                height: Some(300.0),
                actions: vec![],
            })
        );
    }

    #[test]
    fn height_reported_before_showing_releases_at_once() {
        let mut s = Sizing::new();
        let (seq, _) = s.content_sent();
        assert_eq!(
            s.measured(seq, 90.0),
            Some(Release {
                height: Some(90.0),
                actions: vec![],
            })
        );
        assert_eq!(
            s.hold(ACTIVATE),
            Ok(Release {
                height: Some(90.0),
                actions: vec![ACTIVATE],
            })
        );
    }

    #[test]
    fn newer_content_supersedes_an_older_report() {
        let mut s = Sizing::new();
        let (loading, _) = s.content_sent();
        assert_eq!(s.hold(SHOW), Err(true));
        // The result arrives before the page reported "capturing...".
        let (result, timer) = s.content_sent();
        assert!(timer, "the held actions now wait for the new content");
        assert_eq!(s.hold(ACTIVATE), Err(false));
        assert_eq!(s.measured(loading, 60.0), None);
        assert_eq!(s.timed_out(loading), None);
        assert_eq!(
            s.measured(result, 400.0),
            Some(Release {
                height: Some(400.0),
                actions: vec![SHOW, ACTIVATE],
            })
        );
    }

    #[test]
    fn hiding_drops_what_waits() {
        let mut s = Sizing::new();
        let (seq, _) = s.content_sent();
        assert_eq!(s.hold(SHOW), Err(true));
        s.reset();
        assert_eq!(s.timed_out(seq), None);
        assert_eq!(
            s.measured(seq, 100.0),
            Some(Release {
                height: Some(100.0),
                actions: vec![],
            })
        );
    }

    #[test]
    fn a_content_change_while_shown_only_resizes() {
        let mut s = Sizing::new();
        let (first, _) = s.content_sent();
        assert!(s.hold(SHOW).is_err());
        assert!(s.measured(first, 72.0).is_some());
        // "Release the hotkey" replaces "capturing..." in the shown window.
        let (hint, timer) = s.content_sent();
        assert!(!timer);
        assert_eq!(
            s.measured(hint, 96.0),
            Some(Release {
                height: Some(96.0),
                actions: vec![],
            })
        );
    }
}
