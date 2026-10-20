//! Double Ctrl+C detection (D012, `docs/specs/m1-double-copy.md`), platform independent.
//!
//! The platform side listens for clipboard updates, reads what each one holds (see
//! [`CopiedContent`]) and feeds them to a [`DoubleCopyDetector`] with the time it got the
//! notification. The detector says when two copies form a double Ctrl+C:
//! - updates less than [`SAME_COPY`] apart are one copy (some programs write the
//!   clipboard more than once per copy); the copy holds what the last of them read;
//! - a copy that starts [`SAME_COPY`] to [`MAX_GAP`] after the previous one ended is the
//!   second press;
//! - both must be the same text (D012 supplement): one selection copied twice always is,
//!   and a program that writes something else shortly after does not trigger;
//! - VS Code's whole-line copies without a selection (D043) count as "nothing selected";
//! - updates caused by LexTrail's own clipboard fallback are ignored, from the start of
//!   a hotkey capture until [`OWN_COPY_GRACE`] after the last capture in progress
//!   finished (two quick hotkey presses overlap, R1-5).

use std::time::{Duration, Instant};

/// Clipboard updates closer than this belong to one copy.
pub const SAME_COPY: Duration = Duration::from_millis(50);
/// The longest gap between the two copies of a double Ctrl+C.
pub const MAX_GAP: Duration = Duration::from_millis(500);
/// How long after LexTrail's own capture finished its clipboard updates are still
/// ignored: the restore's notification can arrive a little later.
pub const OWN_COPY_GRACE: Duration = Duration::from_millis(300);
/// An own capture that never reported its end stops suppressing after this long (the
/// clipboard fallback gives up after about 3.5 s, D043).
pub const OWN_COPY_MAX: Duration = Duration::from_secs(10);

/// What one clipboard update holds, as far as double Ctrl+C is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopiedContent {
    /// Unicode text, as copied.
    Text(String),
    /// VS Code copied the whole line because nothing was selected (D043); the line.
    EmptySelectionLine(String),
    /// No text (an image, files, ...), or the clipboard could not be read.
    Other,
}

/// A double Ctrl+C.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DoubleCopy {
    /// The text copied twice: look it up.
    Text(String),
    /// Pressed twice with nothing selected (VS Code's whole-line copy).
    NoSelection,
}

/// One copy: one or more clipboard updates less than [`SAME_COPY`] apart.
#[derive(Debug)]
struct Copy {
    last_at: Instant,
    content: CopiedContent,
    /// The previous copy's content, when this copy started within [`MAX_GAP`] of it.
    after: Option<CopiedContent>,
    /// This copy completed a double Ctrl+C; it cannot start another one.
    fired: bool,
}

/// Follows clipboard updates and reports double Ctrl+C presses. Not thread safe by
/// itself; the platform side owns one and feeds it from its listener.
#[derive(Debug, Default)]
pub struct DoubleCopyDetector {
    last: Option<Copy>,
    /// Hotkey captures in progress: their id and start.
    own: Vec<(u64, Instant)>,
    /// When the last own capture in progress finished.
    own_finished: Option<Instant>,
}

impl DoubleCopyDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Hotkey capture `id` (the caller's lookup number) started at `at`; its clipboard
    /// fallback may copy and restore. Updates are ignored from now until
    /// [`OWN_COPY_GRACE`] after the last capture in progress finished
    /// ([`Self::own_copy_finished`]), and a copy made before cannot pair with one made
    /// after.
    pub fn own_copy_started(&mut self, id: u64, at: Instant) {
        self.own.retain(|&(other, _)| other != id);
        self.own.push((id, at));
        self.own_finished = None;
        self.last = None;
    }

    /// Hotkey capture `id` finished at `at` (clipboard restored). Unknown ids (already
    /// finished, or given up on after [`OWN_COPY_MAX`]) are ignored.
    pub fn own_copy_finished(&mut self, id: u64, at: Instant) {
        let before = self.own.len();
        self.own.retain(|&(other, _)| other != id);
        if self.own.is_empty() && self.own.len() < before {
            self.own_finished = Some(at);
        }
    }

    /// A clipboard update arrived at `at` holding `content`. Returns the double Ctrl+C
    /// it completes, if any. Calls must come in time order.
    pub fn on_update(&mut self, at: Instant, content: CopiedContent) -> Option<DoubleCopy> {
        if self.suppressed(at) {
            self.last = None;
            return None;
        }
        let mut copy = match self.last.take() {
            // More of the same copy: what it holds now is what counts.
            Some(mut copy) if at.saturating_duration_since(copy.last_at) < SAME_COPY => {
                copy.last_at = at;
                copy.content = content;
                copy
            }
            Some(prev) if at.saturating_duration_since(prev.last_at) <= MAX_GAP && !prev.fired => {
                Copy {
                    last_at: at,
                    content,
                    after: Some(prev.content),
                    fired: false,
                }
            }
            _ => Copy {
                last_at: at,
                content,
                after: None,
                fired: false,
            },
        };
        let found = match (&copy.after, copy.fired) {
            (Some(first), false) => pair(first, &copy.content),
            _ => None,
        };
        copy.fired |= found.is_some();
        self.last = Some(copy);
        found
    }

    /// Whether an update arriving at `at` falls in an own capture's time and is ignored
    /// anyway: the platform side then need not read the clipboard at all (reading makes
    /// programs that render on demand produce their data, spec section 5).
    /// [`Self::on_update`] checks the same; this only lets the read be skipped.
    pub fn ignores(&mut self, at: Instant) -> bool {
        let ignored = self.suppressed(at);
        if ignored {
            self.last = None;
        }
        ignored
    }

    /// An update that must not count at all (marked "do not monitor" or "not for the
    /// clipboard history", or made in LexTrail's own window): it is not a copy, and the
    /// copy before it cannot pair with the one after.
    pub fn discard(&mut self) {
        self.last = None;
    }

    fn suppressed(&mut self, at: Instant) -> bool {
        // A capture that never reported its end stops counting.
        self.own
            .retain(|&(_, started)| at.saturating_duration_since(started) < OWN_COPY_MAX);
        if !self.own.is_empty() {
            return true;
        }
        match self.own_finished {
            Some(end) if at < end + OWN_COPY_GRACE => true,
            _ => {
                self.own_finished = None;
                false
            }
        }
    }
}

/// Whether the selection UI Automation reports holds the copied text: the same words,
/// whitespace and line endings aside (a copy may turn line breaks into `\r\n`). When it
/// does not, the copied text is looked up without context, never UIA's text instead.
pub(crate) fn selection_holds(copied: &str, selected: &str) -> bool {
    !copied.trim().is_empty() && copied.split_whitespace().eq(selected.split_whitespace())
}

/// Whether two consecutive copies make a double Ctrl+C, and which kind.
fn pair(first: &CopiedContent, second: &CopiedContent) -> Option<DoubleCopy> {
    match (first, second) {
        (CopiedContent::Text(a), CopiedContent::Text(b)) if a == b && !a.trim().is_empty() => {
            Some(DoubleCopy::Text(b.clone()))
        }
        (CopiedContent::EmptySelectionLine(a), CopiedContent::EmptySelectionLine(b)) if a == b => {
            Some(DoubleCopy::NoSelection)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> CopiedContent {
        CopiedContent::Text(s.to_owned())
    }

    fn line(s: &str) -> CopiedContent {
        CopiedContent::EmptySelectionLine(s.to_owned())
    }

    /// Feeds `(ms, content)` updates from one start time; returns each update's result.
    fn run(
        detector: &mut DoubleCopyDetector,
        t0: Instant,
        updates: &[(u64, CopiedContent)],
    ) -> Vec<Option<DoubleCopy>> {
        updates
            .iter()
            .map(|(ms, c)| detector.on_update(t0 + Duration::from_millis(*ms), c.clone()))
            .collect()
    }

    fn found(word: &str) -> Option<DoubleCopy> {
        Some(DoubleCopy::Text(word.to_owned()))
    }

    #[test]
    fn two_copies_of_the_same_text_within_the_gap() {
        for gap in [50, 120, 300, 500] {
            let mut d = DoubleCopyDetector::new();
            let r = run(
                &mut d,
                Instant::now(),
                &[(0, text("launch")), (gap, text("launch"))],
            );
            assert_eq!(r, vec![None, found("launch")], "gap {gap} ms");
        }
    }

    #[test]
    fn too_far_apart_is_two_single_copies() {
        let mut d = DoubleCopyDetector::new();
        let r = run(&mut d, Instant::now(), &[(0, text("a")), (501, text("a"))]);
        assert_eq!(r, vec![None, None]);
        // The second copy can still start a pair of its own.
        let t0 = Instant::now();
        let mut d = DoubleCopyDetector::new();
        let r = run(
            &mut d,
            t0,
            &[(0, text("a")), (900, text("a")), (1100, text("a"))],
        );
        assert_eq!(r, vec![None, None, found("a")]);
    }

    #[test]
    fn updates_closer_than_50ms_are_one_copy() {
        // One copy written in three updates (formats added separately) is not a double.
        let mut d = DoubleCopyDetector::new();
        let r = run(
            &mut d,
            Instant::now(),
            &[(0, text("a")), (10, text("a")), (49, text("a"))],
        );
        assert_eq!(r, vec![None, None, None]);
        // The gap counts from the last update of the first copy: 49 + 451 = 500.
        let mut d = DoubleCopyDetector::new();
        let t0 = Instant::now();
        let r = run(
            &mut d,
            t0,
            &[(0, text("a")), (49, text("a")), (549, text("a"))],
        );
        assert_eq!(r, vec![None, None, found("a")]);
        let r = run(
            &mut DoubleCopyDetector::new(),
            t0,
            &[(0, text("a")), (49, text("a")), (550, text("a"))],
        );
        assert_eq!(r, vec![None, None, None]);
    }

    #[test]
    fn a_burst_holds_what_its_last_update_read() {
        // The second copy's text arrives in its second update.
        let mut d = DoubleCopyDetector::new();
        let r = run(
            &mut d,
            Instant::now(),
            &[
                (0, text("a")),
                (200, CopiedContent::Other),
                (210, text("a")),
            ],
        );
        assert_eq!(r, vec![None, None, found("a")]);
        // The first copy ends up as something else.
        let mut d = DoubleCopyDetector::new();
        let r = run(
            &mut d,
            Instant::now(),
            &[(0, text("a")), (20, CopiedContent::Other), (200, text("a"))],
        );
        assert_eq!(r, vec![None, None, None]);
    }

    #[test]
    fn the_second_copy_must_be_the_same_text() {
        let t0 = Instant::now();
        let r = run(
            &mut DoubleCopyDetector::new(),
            t0,
            &[(0, text("a")), (200, text("b"))],
        );
        assert_eq!(r, vec![None, None]);
        let r = run(
            &mut DoubleCopyDetector::new(),
            t0,
            &[(0, text("a")), (200, text("A"))],
        );
        assert_eq!(r, vec![None, None]);
        let r = run(
            &mut DoubleCopyDetector::new(),
            t0,
            &[(0, text("a")), (200, CopiedContent::Other)],
        );
        assert_eq!(r, vec![None, None]);
        let r = run(
            &mut DoubleCopyDetector::new(),
            t0,
            &[(0, CopiedContent::Other), (200, CopiedContent::Other)],
        );
        assert_eq!(r, vec![None, None]);
        // Blank text is not something to look up.
        let r = run(
            &mut DoubleCopyDetector::new(),
            t0,
            &[(0, text(" \n")), (200, text(" \n"))],
        );
        assert_eq!(r, vec![None, None]);
    }

    #[test]
    fn a_third_copy_does_not_fire_again() {
        let mut d = DoubleCopyDetector::new();
        let r = run(
            &mut d,
            Instant::now(),
            &[
                (0, text("a")),
                (150, text("a")),
                (300, text("a")),
                (450, text("a")),
            ],
        );
        // The third starts a new pair with the fourth.
        assert_eq!(r, vec![None, found("a"), None, found("a")]);
    }

    #[test]
    fn a_fired_copy_ignores_its_own_later_updates() {
        let mut d = DoubleCopyDetector::new();
        let r = run(
            &mut d,
            Instant::now(),
            &[(0, text("a")), (150, text("a")), (160, text("a"))],
        );
        assert_eq!(r, vec![None, found("a"), None]);
    }

    #[test]
    fn vscode_whole_line_copies_mean_nothing_selected() {
        let t0 = Instant::now();
        let r = run(
            &mut DoubleCopyDetector::new(),
            t0,
            &[(0, line("let x = 1;\n")), (200, line("let x = 1;\n"))],
        );
        assert_eq!(r, vec![None, Some(DoubleCopy::NoSelection)]);
        // Mixed with a real selection: not a double.
        let r = run(
            &mut DoubleCopyDetector::new(),
            t0,
            &[(0, text("let x = 1;\n")), (200, line("let x = 1;\n"))],
        );
        assert_eq!(r, vec![None, None]);
        let r = run(
            &mut DoubleCopyDetector::new(),
            t0,
            &[(0, line("let x = 1;\n")), (200, text("let x = 1;\n"))],
        );
        assert_eq!(r, vec![None, None]);
    }

    #[test]
    fn own_capture_updates_are_ignored() {
        // Hotkey capture through the clipboard: the simulated copy and the restore are
        // 50-500 ms apart and would otherwise look like a double copy.
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut d = DoubleCopyDetector::new();
        d.own_copy_started(1, ms(0));
        assert_eq!(d.on_update(ms(160), text("a")), None);
        assert_eq!(d.on_update(ms(400), text("a")), None);
        d.own_copy_finished(1, ms(410));
        // The restore's notification arriving late is still ignored.
        assert_eq!(d.on_update(ms(600), text("a")), None);
        // After the grace period, copies count again, and nothing pairs with the above.
        assert_eq!(d.on_update(ms(710), text("a")), None);
        assert_eq!(d.on_update(ms(900), text("a")), found("a"));
    }

    #[test]
    fn a_copy_before_the_hotkey_does_not_pair_with_one_after() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut d = DoubleCopyDetector::new();
        assert_eq!(d.on_update(ms(0), text("a")), None);
        d.own_copy_started(1, ms(50));
        d.own_copy_finished(1, ms(60));
        assert_eq!(d.on_update(ms(400), text("a")), None);
    }

    #[test]
    fn an_own_capture_without_an_end_stops_suppressing() {
        let t0 = Instant::now();
        let mut d = DoubleCopyDetector::new();
        d.own_copy_started(1, t0);
        let late = t0 + OWN_COPY_MAX;
        assert_eq!(d.on_update(late, text("a")), None);
        assert_eq!(
            d.on_update(late + Duration::from_millis(200), text("a")),
            found("a")
        );
    }

    #[test]
    fn overlapping_own_captures_suppress_until_the_last_one_ends() {
        // Two quick hotkey presses (R1-5): the first capture ending must not end the
        // second one's suppression early.
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut d = DoubleCopyDetector::new();
        d.own_copy_started(1, ms(0));
        d.own_copy_started(2, ms(100));
        d.own_copy_finished(1, ms(300));
        // Long after the first one's grace period, the second's copy and restore.
        assert_eq!(d.on_update(ms(700), text("a")), None);
        assert_eq!(d.on_update(ms(900), text("a")), None);
        d.own_copy_finished(2, ms(950));
        assert_eq!(d.on_update(ms(1200), text("a")), None);
        // Only then do copies count again.
        assert_eq!(d.on_update(ms(1250), text("a")), None);
        assert_eq!(d.on_update(ms(1450), text("a")), found("a"));
    }

    #[test]
    fn the_second_capture_may_finish_first() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut d = DoubleCopyDetector::new();
        d.own_copy_started(1, ms(0));
        d.own_copy_started(2, ms(100));
        d.own_copy_finished(2, ms(300));
        assert_eq!(d.on_update(ms(700), text("a")), None);
        d.own_copy_finished(1, ms(800));
        // A repeated or unknown end changes nothing.
        d.own_copy_finished(1, ms(900));
        d.own_copy_finished(7, ms(900));
        assert_eq!(d.on_update(ms(1050), text("a")), None);
        assert_eq!(d.on_update(ms(1100), text("a")), None);
        assert_eq!(d.on_update(ms(1300), text("a")), found("a"));
    }

    #[test]
    fn a_capture_without_an_end_does_not_hold_up_the_others() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut d = DoubleCopyDetector::new();
        d.own_copy_started(1, ms(0));
        d.own_copy_started(2, ms(100));
        d.own_copy_finished(2, ms(200));
        // Capture 1 never ends: suppressed until OWN_COPY_MAX after its start.
        assert_eq!(d.on_update(ms(5000), text("a")), None);
        let late = ms(0) + OWN_COPY_MAX;
        assert_eq!(d.on_update(late, text("a")), None);
        assert_eq!(
            d.on_update(late + Duration::from_millis(200), text("a")),
            found("a")
        );
    }

    #[test]
    fn ignores_tells_the_own_capture_time_and_breaks_pairs() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut d = DoubleCopyDetector::new();
        assert!(!d.ignores(ms(0)));
        assert_eq!(d.on_update(ms(0), text("a")), None);
        d.own_copy_started(1, ms(100));
        assert!(d.ignores(ms(150)));
        d.own_copy_finished(1, ms(200));
        assert!(d.ignores(ms(499)));
        assert!(!d.ignores(ms(500)));
        // The copy at 0 ms was dropped when the capture started.
        assert_eq!(d.on_update(ms(510), text("a")), None);
        assert_eq!(d.on_update(ms(700), text("a")), found("a"));
    }

    #[test]
    fn the_selection_must_hold_the_copied_text() {
        assert!(selection_holds("launch", "launch"));
        assert!(selection_holds("two\r\nlines ", "two\nlines"));
        assert!(selection_holds("a\u{a0}b", "a b"));
        assert!(!selection_holds("launch", "launches"));
        assert!(!selection_holds("launch", "Launch"));
        assert!(!selection_holds("launch", "launch missiles"));
        assert!(!selection_holds(" ", " "));
    }

    #[test]
    fn a_discarded_update_breaks_a_pair() {
        // A password manager's marked copy between two copies of the same text.
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut d = DoubleCopyDetector::new();
        assert_eq!(d.on_update(ms(0), text("a")), None);
        d.discard();
        assert_eq!(d.on_update(ms(200), text("a")), None);
        // The copy after it pairs normally with the next one.
        assert_eq!(d.on_update(ms(400), text("a")), found("a"));
    }
}
