//! The settings window (D047): opened from the tray's "设置…" (or `--settings`), created
//! on demand and destroyed when closed, sharing the app's WebView2 environment (D010).
//! Its commands are the only ones its capability allows (`capabilities/settings.json`).

use lextrail_config::hotkey::{self, Hotkey, Modifiers, Verdict};
use lextrail_config::{HotkeySettings, Settings};
use serde::Serialize;
use tauri::webview::PageLoadEvent;
use tauri::window::Color;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::hotkey::{Applied, LookupHotkey};
use crate::settings::SettingsService;

pub const LABEL: &str = "settings";

const WIDTH: f64 = 720.0;
const HEIGHT: f64 = 520.0;
const MIN_WIDTH: f64 = 600.0;
const MIN_HEIGHT: f64 = 420.0;
/// Matches the page background so the window never flashes white.
const BACKGROUND: Color = Color(255, 255, 255, 255);

/// Brings the settings window to the front, creating it if it does not exist.
pub fn open<R: Runtime>(app: &AppHandle<R>) {
    if let Err(e) = try_open(app) {
        eprintln!("failed to open settings window: {e}");
    }
}

fn try_open<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(LABEL) {
        // Still loading: the page-load handler shows it.
        if !window.is_visible()? {
            return Ok(());
        }
        if window.is_minimized()? {
            window.unminimize()?;
        }
        window.show()?;
        return window.set_focus();
    }

    let window =
        WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("settings/index.html".into()))
            .title("LexTrail 设置")
            .inner_size(WIDTH, HEIGHT)
            .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
            .center()
            .background_color(BACKGROUND)
            .visible(false)
            .on_page_load(|window, payload| {
                if payload.event() == PageLoadEvent::Finished
                    && !window.is_visible().unwrap_or(true)
                {
                    let shown = window.show().and_then(|()| window.set_focus());
                    if let Err(e) = shown {
                        eprintln!("failed to show settings window: {e}");
                    }
                }
            })
            .build()?;
    // However the window goes away, recording must not leave the hotkey off (spec 4.5).
    let handle = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            crate::hotkey::with_state(&handle, |state, registrar| {
                state.cancel_recording(registrar);
            });
        }
    });
    Ok(())
}

/// A hotkey as the page shows it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyView {
    accelerator: String,
    display: String,
}

impl From<&Hotkey> for HotkeyView {
    fn from(hotkey: &Hotkey) -> Self {
        Self {
            accelerator: hotkey.canonical(),
            display: hotkey.display(),
        }
    }
}

/// Everything the settings page shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    /// `None` when the settings file's value cannot be used.
    lookup: Option<HotkeyView>,
    /// The user chose no lookup hotkey (spec 5.4).
    lookup_off: bool,
    lookup_active: bool,
    /// Why the lookup hotkey is not working.
    lookup_error: Option<String>,
    default_lookup: HotkeyView,
    double_ctrl_c: bool,
    record_history: bool,
    /// Why nothing can be changed here, if so.
    read_only: Option<String>,
}

fn view(settings: &Settings, lookup: &LookupHotkey, read_only: Option<String>) -> SettingsView {
    let default = Hotkey::parse(&HotkeySettings::default().lookup).expect("valid default");
    SettingsView {
        lookup: lookup.configured().map(HotkeyView::from),
        lookup_off: lookup.is_off(),
        lookup_active: lookup.is_active(),
        lookup_error: if lookup.is_active() {
            None
        } else {
            lookup.error().map(str::to_owned)
        },
        default_lookup: HotkeyView::from(&default),
        double_ctrl_c: settings.hotkeys.double_ctrl_c,
        record_history: settings.history.record,
        read_only,
    }
}

fn current_view<R: Runtime>(app: &AppHandle<R>) -> SettingsView {
    let service = app.state::<SettingsService>();
    let settings = service.settings();
    let read_only = service.read_only();
    crate::hotkey::with_state(app, |state, _| view(&settings, state, read_only))
}

#[tauri::command]
pub fn settings_get<R: Runtime>(app: AppHandle<R>) -> SettingsView {
    current_view(&app)
}

/// Recording starts (the hotkey is turned off) or is cancelled (it comes back).
#[tauri::command]
pub fn hotkey_recording<R: Runtime>(app: AppHandle<R>, active: bool) -> SettingsView {
    crate::hotkey::with_state(&app, |state, registrar| {
        if active {
            state.start_recording(registrar);
        } else {
            state.cancel_recording(registrar);
        }
    });
    current_view(&app)
}

/// One key press while recording.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub struct KeyPress {
    ctrl: bool,
    alt: bool,
    shift: bool,
    meta: bool,
}

/// The result of checking a recorded key press.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CheckView {
    hotkey: HotkeyView,
    /// Cannot be used, and why.
    blocked: Option<String>,
}

fn check_key(code: &str, keys: KeyPress) -> Option<CheckView> {
    let modifiers = Modifiers {
        ctrl: keys.ctrl,
        alt: keys.alt,
        shift: keys.shift,
        win: keys.meta,
    };
    let hotkey = Hotkey::from_key_event(code, modifiers)?;
    let blocked = match hotkey::check(&hotkey) {
        Verdict::Blocked(why) => Some(why),
        Verdict::Allowed => None,
    };
    Some(CheckView {
        hotkey: HotkeyView::from(&hotkey),
        blocked,
    })
}

/// `None`: `code` is a modifier or an unsupported key; keep recording.
#[tauri::command]
pub fn hotkey_check(code: String, keys: KeyPress) -> Option<CheckView> {
    check_key(&code, keys)
}

/// What changing the hotkey did, plus the page's new state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyView {
    /// `changed`, `unchanged`, `blocked`, `register_failed`, `save_failed`.
    outcome: &'static str,
    message: Option<String>,
    settings: SettingsView,
}

/// Changes the lookup hotkey; an empty `accelerator` turns it off (spec 5.4).
#[tauri::command]
pub fn hotkey_apply<R: Runtime>(
    app: AppHandle<R>,
    accelerator: String,
) -> Result<ApplyView, String> {
    let new = if accelerator.is_empty() {
        None
    } else {
        Some(Hotkey::parse(&accelerator).map_err(|e| e.to_string())?)
    };
    let service = app.state::<SettingsService>();
    let applied = crate::hotkey::with_state(&app, |state, registrar| {
        state.apply(registrar, new, |hotkey| {
            let text = hotkey.map(Hotkey::canonical).unwrap_or_default();
            service.update(|s| s.hotkeys.lookup = text)
        })
    });
    #[cfg(debug_assertions)]
    eprintln!("settings: hotkey {accelerator}: {applied:?}");
    let (outcome, message) = match applied {
        Applied::Changed => ("changed", None),
        Applied::Unchanged => ("unchanged", None),
        Applied::Blocked(why) => ("blocked", Some(why)),
        Applied::RegisterFailed(why) => ("register_failed", Some(why)),
        Applied::SaveFailed(why) => ("save_failed", Some(why)),
    };
    Ok(ApplyView {
        outcome,
        message,
        settings: current_view(&app),
    })
}

/// Double Ctrl+C on / off, right away (spec 5.2): on starts the listener before saving,
/// off saves before stopping it, so a failure leaves both as they were.
#[tauri::command]
pub fn settings_set_double_ctrl_c<R: Runtime>(
    app: AppHandle<R>,
    on: bool,
) -> Result<SettingsView, String> {
    let service = app.state::<SettingsService>();
    if on {
        crate::popup::set_double_copy(&app, true)
            .map_err(|e| format!("无法启动剪贴板监听：{e}"))?;
        if let Err(e) = service.update(|s| s.hotkeys.double_ctrl_c = true) {
            let _ = crate::popup::set_double_copy(&app, false);
            return Err(e);
        }
    } else {
        service.update(|s| s.hotkeys.double_ctrl_c = false)?;
        let _ = crate::popup::set_double_copy(&app, false);
    }
    Ok(current_view(&app))
}

/// Record the lookup history or not, from the next lookup on (spec 5.2).
#[tauri::command]
pub fn settings_set_record_history<R: Runtime>(
    app: AppHandle<R>,
    record: bool,
) -> Result<SettingsView, String> {
    app.state::<SettingsService>()
        .update(|s| s.history.record = record)?;
    crate::popup::set_record_history(&app, record);
    Ok(current_view(&app))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTRL_ALT: KeyPress = KeyPress {
        ctrl: true,
        alt: true,
        shift: false,
        meta: false,
    };

    #[test]
    fn modifier_presses_keep_recording() {
        assert_eq!(check_key("ControlLeft", CTRL_ALT), None);
        assert_eq!(check_key("AltLeft", CTRL_ALT), None);
    }

    #[test]
    fn recorded_key_is_checked() {
        let checked = check_key("KeyY", CTRL_ALT).unwrap();
        assert_eq!(checked.hotkey.accelerator, "Ctrl+Alt+Y");
        assert_eq!(checked.hotkey.display, "Ctrl + Alt + Y");
        assert_eq!(checked.blocked, None);

        let ctrl = KeyPress {
            alt: false,
            ..CTRL_ALT
        };
        assert!(check_key("KeyC", ctrl).unwrap().blocked.is_some());
        // Other programs' shortcuts are the user's choice (D048).
        assert_eq!(check_key("KeyV", ctrl).unwrap().blocked, None);
        let win = KeyPress {
            meta: true,
            ..CTRL_ALT
        };
        assert!(
            check_key("KeyQ", win)
                .unwrap()
                .blocked
                .unwrap()
                .contains("Win")
        );
    }

    #[test]
    fn view_reports_a_failed_hotkey() {
        struct Taken;
        impl crate::hotkey::Registrar for Taken {
            fn register(&mut self, _: &Hotkey) -> Result<(), String> {
                Err("占用".into())
            }
            fn unregister(&mut self, _: &Hotkey) {}
        }
        let state = LookupHotkey::startup("Ctrl+Alt+X", &mut Taken);
        let v = view(&Settings::default(), &state, None);
        assert_eq!(v.lookup.unwrap().display, "Ctrl + Alt + X");
        assert!(!v.lookup_active);
        assert_eq!(v.lookup_error.as_deref(), Some("占用"));
        assert_eq!(v.default_lookup.accelerator, "Ctrl+Alt+X");
        assert!(v.record_history && !v.double_ctrl_c);
        assert!(!v.lookup_off);
    }

    #[test]
    fn view_reports_no_hotkey() {
        let state = LookupHotkey::startup("", &mut NeverCalled);
        let v = view(&Settings::default(), &state, None);
        assert!(v.lookup_off && v.lookup.is_none() && v.lookup_error.is_none());
    }

    struct NeverCalled;
    impl crate::hotkey::Registrar for NeverCalled {
        fn register(&mut self, _: &Hotkey) -> Result<(), String> {
            panic!("nothing to register")
        }
        fn unregister(&mut self, _: &Hotkey) {}
    }
}
