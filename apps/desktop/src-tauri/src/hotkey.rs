//! Global hotkeys: registration and reporting registration failures (D014).

use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Something a global hotkey can trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    /// Capture the current selection and show the popup.
    Lookup,
}

impl HotkeyAction {
    pub const ALL: [Self; 1] = [Self::Lookup];

    /// Provisional default (D014): one-handed with the left hand and clear of the most
    /// common application shortcuts. The user confirms or changes it on first launch.
    pub fn default_accelerator(self) -> &'static str {
        match self {
            Self::Lookup => "Ctrl+Alt+X",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Lookup => "划词翻译",
        }
    }
}

/// An accelerator string assigned to an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub action: HotkeyAction,
    pub accelerator: String,
}

pub fn default_bindings() -> Vec<Binding> {
    HotkeyAction::ALL
        .into_iter()
        .map(|action| Binding {
            action,
            accelerator: action.default_accelerator().to_owned(),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureReason {
    /// The accelerator string could not be parsed.
    Invalid,
    /// The OS refused the registration, usually because another program holds it.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub binding: Binding,
    pub reason: FailureReason,
    pub detail: String,
}

/// Registers every binding and returns the ones that failed. A failure never aborts
/// the remaining registrations.
pub fn register_all<R: Runtime>(app: &AppHandle<R>, bindings: &[Binding]) -> Vec<Failure> {
    bindings
        .iter()
        .filter_map(|binding| register(app, binding).err())
        .collect()
}

fn register<R: Runtime>(app: &AppHandle<R>, binding: &Binding) -> Result<(), Failure> {
    let failure = |reason, detail: String| Failure {
        binding: binding.clone(),
        reason,
        detail,
    };
    let shortcut = binding
        .accelerator
        .parse::<Shortcut>()
        .map_err(|e| failure(FailureReason::Invalid, e.to_string()))?;
    let action = binding.action;
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                on_triggered(app, action);
            }
        })
        .map_err(|e| failure(FailureReason::Unavailable, e.to_string()))
}

fn on_triggered<R: Runtime>(_app: &AppHandle<R>, action: HotkeyAction) {
    match action {
        // Wired to capture and the popup in the popup task.
        HotkeyAction::Lookup => {
            #[cfg(debug_assertions)]
            eprintln!("hotkey triggered: {action:?}");
        }
    }
}

/// User-facing summary of registration failures, or `None` when everything registered.
pub fn failure_message(failures: &[Failure]) -> Option<String> {
    if failures.is_empty() {
        return None;
    }
    let lines: Vec<String> = failures
        .iter()
        .map(|f| {
            let why = match f.reason {
                FailureReason::Invalid => "快捷键格式无效",
                FailureReason::Unavailable => "可能已被其他程序占用",
            };
            format!(
                "「{}」的快捷键 {} 注册失败：{}。",
                f.binding.action.label(),
                f.binding.accelerator,
                why
            )
        })
        .collect();
    Some(format!(
        "{}\n\n请关闭占用该快捷键的程序后重启 LexTrail，或在设置中更换快捷键。",
        lines.join("\n")
    ))
}

/// Tells the user about failed registrations without opening a WebView: a native
/// message box plus a tray tooltip that stays until the next launch.
pub fn notify_failures<R: Runtime>(app: &AppHandle<R>, failures: &[Failure]) {
    let Some(message) = failure_message(failures) else {
        return;
    };
    eprintln!("hotkey registration failed: {failures:?}");
    if let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID) {
        let _ = tray.set_tooltip(Some("LexTrail（快捷键注册失败）"));
    }
    app.dialog()
        .message(message)
        .title("LexTrail 快捷键注册失败")
        .kind(MessageDialogKind::Warning)
        .show(|_| {});
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_accelerators_parse() {
        for binding in default_bindings() {
            assert!(
                binding.accelerator.parse::<Shortcut>().is_ok(),
                "{} should parse",
                binding.accelerator
            );
        }
    }

    #[test]
    fn default_accelerators_are_unique() {
        let mut ids: Vec<u32> = default_bindings()
            .iter()
            .map(|b| b.accelerator.parse::<Shortcut>().unwrap().id())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), HotkeyAction::ALL.len());
    }

    #[test]
    fn lookup_default_is_ctrl_alt_x() {
        // D014: provisional default for lookup.
        let expected: Shortcut = "ctrl+alt+KeyX".parse().unwrap();
        let actual: Shortcut = HotkeyAction::Lookup.default_accelerator().parse().unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn invalid_accelerator_does_not_parse() {
        assert!("Ctrl+Alt+NotAKey".parse::<Shortcut>().is_err());
        assert!("".parse::<Shortcut>().is_err());
    }

    #[test]
    fn no_message_when_everything_registered() {
        assert_eq!(failure_message(&[]), None);
    }

    #[test]
    fn message_names_action_accelerator_and_reason() {
        let failures = [
            Failure {
                binding: Binding {
                    action: HotkeyAction::Lookup,
                    accelerator: "Ctrl+Alt+X".into(),
                },
                reason: FailureReason::Unavailable,
                detail: "os error".into(),
            },
            Failure {
                binding: Binding {
                    action: HotkeyAction::Lookup,
                    accelerator: "Ctrl+Nope".into(),
                },
                reason: FailureReason::Invalid,
                detail: "parse error".into(),
            },
        ];
        let message = failure_message(&failures).unwrap();
        assert!(message.contains("划词翻译"));
        assert!(message.contains("Ctrl+Alt+X"));
        assert!(message.contains("已被其他程序占用"));
        assert!(message.contains("Ctrl+Nope"));
        assert!(message.contains("格式无效"));
    }
}
