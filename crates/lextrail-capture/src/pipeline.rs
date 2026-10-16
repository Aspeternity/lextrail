//! Tries capture methods in order (D011). Platform-independent, so the ordering and
//! result rules are unit-tested without any real windows.

use std::time::Instant;

use crate::{
    CaptureError, CaptureOutcome, CaptureTarget, CaptureTrace, Integrity, Progress, StepContext,
    StepOutput, StepTrace, Unavailable, result_label,
};

/// One capture method (UI Automation, clipboard, ...). Each step enforces its own time
/// budget and reports running out of it as [`Unavailable::TimedOut`].
pub trait CaptureStep: Send + Sync {
    fn name(&self) -> &'static str;

    /// `context` holds what earlier steps learned (e.g. the focused element's class);
    /// `progress` receives the step's [`crate::CaptureProgress`] reports.
    fn try_capture(
        &self,
        target: &CaptureTarget,
        context: &StepContext,
        progress: Progress<'_>,
    ) -> StepOutput;

    /// Whether to run even though an earlier step reported "no selection". By default
    /// "no selection" is final: re-checking a reliable answer only costs time (and, for
    /// the clipboard, a copy). Steps override this for targets whose earlier answer is
    /// known to be unreliable (D043).
    fn confirms_no_selection(&self, _target: &CaptureTarget, _context: &StepContext) -> bool {
        false
    }
}

impl<T: CaptureStep + ?Sized> CaptureStep for std::sync::Arc<T> {
    fn name(&self) -> &'static str {
        (**self).name()
    }

    fn try_capture(
        &self,
        target: &CaptureTarget,
        context: &StepContext,
        progress: Progress<'_>,
    ) -> StepOutput {
        (**self).try_capture(target, context, progress)
    }

    fn confirms_no_selection(&self, target: &CaptureTarget, context: &StepContext) -> bool {
        (**self).confirms_no_selection(target, context)
    }
}

/// Runs [`CaptureStep`]s in order until one captures something.
///
/// Before any step runs:
/// - no foreground window → [`Unavailable::NoForegroundWindow`];
/// - the target is LexTrail itself → [`Unavailable::Unsupported`];
/// - the target runs elevated → [`Unavailable::Elevated`] (no step could succeed).
///
/// A step reporting [`Unavailable::Elevated`] also ends the run. After a step reports
/// [`Unavailable::NoSelection`], later steps run only if they
/// [confirm](CaptureStep::confirms_no_selection) it for this target. When no step captures
/// anything, the result is the most useful of what they reported:
/// `NoSelection` > `KeysHeld` > `TimedOut` > an error > `Unsupported`.
pub struct Pipeline {
    steps: Vec<Box<dyn CaptureStep>>,
    own_process_id: u32,
}

impl Pipeline {
    pub fn new(steps: Vec<Box<dyn CaptureStep>>) -> Self {
        Self::with_own_process_id(steps, std::process::id())
    }

    fn with_own_process_id(steps: Vec<Box<dyn CaptureStep>>, own_process_id: u32) -> Self {
        Self {
            steps,
            own_process_id,
        }
    }

    pub fn run(&self, target: &CaptureTarget) -> Result<CaptureOutcome, CaptureError> {
        self.run_traced(target, &|_| {}).0
    }

    /// [`Pipeline::run`] plus a [`CaptureTrace`] of the steps that ran; the steps report
    /// progress to `progress`.
    pub fn run_traced(
        &self,
        target: &CaptureTarget,
        progress: Progress<'_>,
    ) -> (Result<CaptureOutcome, CaptureError>, CaptureTrace) {
        let start = Instant::now();
        let mut trace = CaptureTrace::default();
        let result = self.run_steps(target, progress, &mut trace);
        trace.total = start.elapsed();
        (result, trace)
    }

    fn run_steps(
        &self,
        target: &CaptureTarget,
        progress: Progress<'_>,
        trace: &mut CaptureTrace,
    ) -> Result<CaptureOutcome, CaptureError> {
        let unavailable = |u| Ok(CaptureOutcome::Unavailable(u));
        let precheck = match &target.window {
            None => Some(Unavailable::NoForegroundWindow),
            Some(w) if w.process_id == self.own_process_id => Some(Unavailable::Unsupported),
            Some(w) if w.integrity == Integrity::Higher => Some(Unavailable::Elevated),
            Some(_) => None,
        };
        if let Some(u) = precheck {
            trace.precheck = Some(u);
            return unavailable(u);
        }

        let mut best: Option<Result<Unavailable, CaptureError>> = None;
        let mut context = StepContext::default();
        for step in &self.steps {
            if context.no_selection && !step.confirms_no_selection(target, &context) {
                continue;
            }
            let start = Instant::now();
            let output = step.try_capture(target, &context, progress);
            if output.focused_class.is_some() {
                context.focused_class.clone_from(&output.focused_class);
            }
            trace.steps.push(StepTrace {
                step: step.name(),
                result: result_label(&output.result),
                elapsed: start.elapsed(),
                element: output.element,
                attempts: output.attempts,
                note: output.note,
                key_wait: output.key_wait,
            });
            match output.result {
                Ok(CaptureOutcome::Captured(selection)) => {
                    return Ok(CaptureOutcome::Captured(selection));
                }
                Ok(CaptureOutcome::Unavailable(Unavailable::Elevated)) => {
                    return unavailable(Unavailable::Elevated);
                }
                Ok(CaptureOutcome::Unavailable(u)) => {
                    context.no_selection |= u == Unavailable::NoSelection;
                    keep_best(&mut best, Ok(u));
                }
                Err(e) => keep_best(&mut best, Err(e)),
            }
        }
        match best {
            Some(Ok(u)) => unavailable(u),
            Some(Err(e)) => Err(e),
            None => unavailable(Unavailable::Unsupported),
        }
    }
}

fn rank(result: &Result<Unavailable, CaptureError>) -> u8 {
    match result {
        Ok(Unavailable::NoSelection) => 5,
        Ok(Unavailable::KeysHeld) => 4,
        Ok(Unavailable::TimedOut) => 3,
        Err(_) => 2,
        Ok(_) => 1,
    }
}

fn keep_best(
    best: &mut Option<Result<Unavailable, CaptureError>>,
    next: Result<Unavailable, CaptureError>,
) {
    // Ties keep the earlier step's result.
    if best.as_ref().is_none_or(|b| rank(&next) > rank(b)) {
        *best = Some(next);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::{CaptureMethod, ElementSource, Selection, WindowInfo};

    struct Fixed {
        result: Result<CaptureOutcome, CaptureError>,
        calls: Arc<AtomicUsize>,
    }

    impl CaptureStep for Fixed {
        fn name(&self) -> &'static str {
            "fixed"
        }

        fn try_capture(&self, _: &CaptureTarget, _: &StepContext, _: Progress<'_>) -> StepOutput {
            self.calls.fetch_add(1, Ordering::SeqCst);
            StepOutput {
                element: Some(ElementSource::HwndFocus),
                ..StepOutput::from(self.result.clone())
            }
        }
    }

    /// Reports a focused class, and records the class it was given by earlier steps.
    struct Probe {
        result: Result<CaptureOutcome, CaptureError>,
        focused_class: Option<&'static str>,
        confirms: bool,
        seen_class: Arc<std::sync::Mutex<Option<Option<String>>>>,
    }

    impl CaptureStep for Probe {
        fn name(&self) -> &'static str {
            "probe"
        }

        fn try_capture(
            &self,
            _: &CaptureTarget,
            context: &StepContext,
            _: Progress<'_>,
        ) -> StepOutput {
            *self.seen_class.lock().unwrap() = Some(context.focused_class.clone());
            StepOutput {
                focused_class: self.focused_class.map(str::to_owned),
                ..StepOutput::from(self.result.clone())
            }
        }

        fn confirms_no_selection(&self, _: &CaptureTarget, _: &StepContext) -> bool {
            self.confirms
        }
    }

    fn probe(result: Result<CaptureOutcome, CaptureError>, confirms: bool) -> Probe {
        Probe {
            result,
            focused_class: None,
            confirms,
            seen_class: Arc::default(),
        }
    }

    fn captured(text: &str) -> Result<CaptureOutcome, CaptureError> {
        Ok(CaptureOutcome::Captured(Selection {
            text: text.into(),
            context: None,
            url: None,
            method: CaptureMethod::Uia,
        }))
    }

    fn unavailable(u: Unavailable) -> Result<CaptureOutcome, CaptureError> {
        Ok(CaptureOutcome::Unavailable(u))
    }

    fn error() -> Result<CaptureOutcome, CaptureError> {
        Err(CaptureError::Platform {
            operation: "test",
            code: 0x8000_4005,
            message: "failed".into(),
        })
    }

    /// Runs steps with the given results; returns the outcome and how often each ran.
    fn run_with(
        target: &CaptureTarget,
        results: Vec<Result<CaptureOutcome, CaptureError>>,
    ) -> (Result<CaptureOutcome, CaptureError>, Vec<usize>) {
        let counters: Vec<Arc<AtomicUsize>> = results
            .iter()
            .map(|_| Arc::new(AtomicUsize::new(0)))
            .collect();
        let steps = results
            .into_iter()
            .zip(&counters)
            .map(|(result, calls)| {
                Box::new(Fixed {
                    result,
                    calls: Arc::clone(calls),
                }) as Box<dyn CaptureStep>
            })
            .collect();
        let outcome = Pipeline::with_own_process_id(steps, 1).run(target);
        let calls = counters.iter().map(|c| c.load(Ordering::SeqCst)).collect();
        (outcome, calls)
    }

    fn target() -> CaptureTarget {
        CaptureTarget::for_tests(Some(WindowInfo::for_tests("notepad.exe")))
    }

    #[test]
    fn first_captured_wins_and_later_steps_do_not_run() {
        let (outcome, calls) = run_with(&target(), vec![captured("a"), captured("b")]);
        assert_eq!(outcome, captured("a"));
        assert_eq!(calls, [1, 0]);
    }

    #[test]
    fn falls_back_after_timeout_unsupported_or_error() {
        // ("No selection" only falls back when the next step confirms it; see
        // no_selection_is_final_unless_the_next_step_confirms_it.)
        for first in [
            unavailable(Unavailable::TimedOut),
            unavailable(Unavailable::Unsupported),
            error(),
        ] {
            let (outcome, calls) = run_with(&target(), vec![first, captured("b")]);
            assert_eq!(outcome, captured("b"));
            assert_eq!(calls, [1, 1]);
        }
    }

    #[test]
    fn reports_the_most_useful_failure() {
        use Unavailable::*;
        let cases = [
            (
                vec![unavailable(Unsupported), unavailable(NoSelection)],
                unavailable(NoSelection),
            ),
            (
                vec![unavailable(TimedOut), unavailable(NoSelection)],
                unavailable(NoSelection),
            ),
            (
                vec![unavailable(TimedOut), unavailable(Unsupported)],
                unavailable(TimedOut),
            ),
            (vec![error(), unavailable(TimedOut)], unavailable(TimedOut)),
            (
                vec![unavailable(TimedOut), unavailable(KeysHeld)],
                unavailable(KeysHeld),
            ),
            (
                vec![unavailable(Unsupported), unavailable(KeysHeld)],
                unavailable(KeysHeld),
            ),
            (vec![unavailable(Unsupported), error()], error()),
            (vec![unavailable(Unsupported)], unavailable(Unsupported)),
            (vec![], unavailable(Unsupported)),
        ];
        for (results, expected) in cases {
            assert_eq!(run_with(&target(), results).0, expected);
        }
    }

    #[test]
    fn elevated_step_result_stops_the_run() {
        let (outcome, calls) = run_with(
            &target(),
            vec![unavailable(Unavailable::Elevated), captured("b")],
        );
        assert_eq!(outcome, unavailable(Unavailable::Elevated));
        assert_eq!(calls, [1, 0]);
    }

    #[test]
    fn prechecks_skip_all_steps() {
        let no_window = CaptureTarget::for_tests(None);
        let (outcome, calls) = run_with(&no_window, vec![captured("a")]);
        assert_eq!(outcome, unavailable(Unavailable::NoForegroundWindow));
        assert_eq!(calls, [0]);

        let mut own = WindowInfo::for_tests("lextrail-desktop.exe");
        own.process_id = 1;
        let (outcome, calls) = run_with(&CaptureTarget::for_tests(Some(own)), vec![captured("a")]);
        assert_eq!(outcome, unavailable(Unavailable::Unsupported));
        assert_eq!(calls, [0]);

        let mut elevated = WindowInfo::for_tests("regedit.exe");
        elevated.integrity = Integrity::Higher;
        let (outcome, calls) = run_with(
            &CaptureTarget::for_tests(Some(elevated)),
            vec![captured("a")],
        );
        assert_eq!(outcome, unavailable(Unavailable::Elevated));
        assert_eq!(calls, [0]);
    }

    #[test]
    fn unknown_integrity_still_tries_steps() {
        let mut window = WindowInfo::for_tests("app.exe");
        window.integrity = Integrity::Unknown;
        let (outcome, _) = run_with(&CaptureTarget::for_tests(Some(window)), vec![captured("a")]);
        assert_eq!(outcome, captured("a"));
    }

    #[test]
    fn trace_records_each_step_without_text() {
        let steps: Vec<Box<dyn CaptureStep>> = vec![
            Box::new(Fixed {
                result: unavailable(Unavailable::TimedOut),
                calls: Arc::default(),
            }),
            Box::new(Fixed {
                result: captured("secret text"),
                calls: Arc::default(),
            }),
        ];
        let (result, trace) =
            Pipeline::with_own_process_id(steps, 1).run_traced(&target(), &|_| {});
        assert_eq!(result, captured("secret text"));
        assert_eq!(trace.precheck, None);
        let labels: Vec<_> = trace
            .steps
            .iter()
            .map(|s| (s.step, s.result, s.element))
            .collect();
        assert_eq!(
            labels,
            [
                ("fixed", "timed_out", Some(ElementSource::HwndFocus)),
                ("fixed", "captured", Some(ElementSource::HwndFocus)),
            ]
        );
        let line = trace.to_string();
        assert!(line.starts_with("fixed=timed_out "), "{line}");
        assert!(line.contains("(hwnd_focus)"), "{line}");
        assert!(!line.contains("secret"), "{line}");
    }

    #[test]
    fn trace_shows_the_key_wait() {
        use std::time::Duration;
        let trace = CaptureTrace {
            steps: vec![StepTrace {
                step: "clipboard",
                result: "captured",
                elapsed: Duration::from_micros(160_200),
                element: None,
                attempts: 1,
                note: Some("ctrl_insert"),
                key_wait: Some(Duration::from_micros(150_100)),
            }],
            precheck: None,
            total: Duration::from_micros(160_300),
        };
        assert_eq!(
            trace.to_string(),
            "clipboard=captured 160.2ms keys=150.1ms [ctrl_insert]; total 160.3ms"
        );
    }

    #[test]
    fn trace_records_prechecks() {
        let (_, trace) = Pipeline::with_own_process_id(vec![], 1)
            .run_traced(&CaptureTarget::for_tests(None), &|_| {});
        assert_eq!(trace.precheck, Some(Unavailable::NoForegroundWindow));
        assert!(trace.steps.is_empty());
        let line = trace.to_string();
        assert!(
            line.starts_with("precheck=no_foreground_window; total "),
            "{line}"
        );
    }

    #[test]
    fn no_selection_is_final_unless_the_next_step_confirms_it() {
        // A step that does not confirm (the default) is skipped after "no selection".
        let (outcome, calls) = run_with(
            &target(),
            vec![unavailable(Unavailable::NoSelection), captured("b")],
        );
        assert_eq!(outcome, unavailable(Unavailable::NoSelection));
        assert_eq!(calls, [1, 0]);

        // A step that confirms runs; its capture wins, and it sees the class the first
        // step reported.
        let seen = Arc::default();
        let steps: Vec<Box<dyn CaptureStep>> = vec![
            Box::new(Probe {
                focused_class: Some("xterm-helper-textarea"),
                ..probe(unavailable(Unavailable::NoSelection), false)
            }),
            Box::new(Probe {
                seen_class: Arc::clone(&seen),
                ..probe(captured("b"), true)
            }),
        ];
        assert_eq!(
            Pipeline::with_own_process_id(steps, 1).run(&target()),
            captured("b")
        );
        assert_eq!(
            *seen.lock().unwrap(),
            Some(Some("xterm-helper-textarea".to_owned()))
        );

        // Confirming and still finding nothing: "no selection".
        let steps: Vec<Box<dyn CaptureStep>> = vec![
            Box::new(probe(unavailable(Unavailable::NoSelection), false)),
            Box::new(probe(unavailable(Unavailable::NoSelection), true)),
        ];
        assert_eq!(
            Pipeline::with_own_process_id(steps, 1).run(&target()),
            unavailable(Unavailable::NoSelection)
        );
    }

    #[test]
    fn unsupported_falls_through_without_confirmation() {
        let steps: Vec<Box<dyn CaptureStep>> = vec![
            Box::new(probe(unavailable(Unavailable::Unsupported), false)),
            Box::new(probe(captured("b"), false)),
        ];
        assert_eq!(
            Pipeline::with_own_process_id(steps, 1).run(&target()),
            captured("b")
        );
    }
}
