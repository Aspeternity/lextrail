//! Global hotkeys (D014): registration at startup, recording and changing the lookup
//! hotkey from the settings window (`docs/specs/m1-hotkey-settings.md`, sections 4–5).
//!
//! [`LookupHotkey`] is the state machine; it reaches the OS through a [`Registrar`], so
//! every path is unit tested with a fake one.

use std::sync::Mutex;
use std::time::Instant;

use lextrail_config::hotkey::{self, Hotkey, Verdict};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// The user-facing name of the lookup hotkey.
const LOOKUP_LABEL: &str = "划词翻译";
const TRAY_TOOLTIP: &str = "LexTrail";
const TRAY_TOOLTIP_FAILED: &str = "LexTrail（快捷键注册失败）";

/// Registers and unregisters global hotkeys. `register` errors are shown to the user.
pub trait Registrar {
    fn register(&mut self, hotkey: &Hotkey) -> Result<(), String>;
    fn unregister(&mut self, hotkey: &Hotkey);
}

/// What [`LookupHotkey::apply`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    /// The new hotkey is registered and saved.
    Changed,
    /// Same as the current one; nothing to save.
    Unchanged,
    /// Cannot be used (spec 3.1); the old one is back.
    Blocked(String),
    /// Has warnings the user has not confirmed yet (spec 3.2); still recording.
    NeedsConfirmation(Vec<String>),
    /// The OS refused the new one; the old one is back.
    RegisterFailed(String),
    /// Saving failed; the new one was dropped and the old one is back.
    SaveFailed(String),
}

/// The lookup hotkey: what the settings say, and whether it is registered right now.
#[derive(Debug)]
pub struct LookupHotkey {
    /// From the settings file; `None` when the file's value cannot be used.
    configured: Option<Hotkey>,
    /// `configured` is registered with the OS.
    active: bool,
    /// Why it is not active (shown in the settings window and the startup notice).
    error: Option<String>,
    /// The settings window is recording: `configured` is unregistered on purpose.
    recording: bool,
}

impl LookupHotkey {
    /// Reads the settings-file value and registers it.
    pub fn startup(text: &str, registrar: &mut impl Registrar) -> Self {
        let mut this = Self {
            configured: None,
            active: false,
            error: None,
            recording: false,
        };
        match Hotkey::parse(text) {
            Err(_) => this.error = Some(format!("设置文件中的快捷键 {text} 格式无效")),
            Ok(hotkey) => match hotkey::check(&hotkey) {
                Verdict::Blocked(why) => this.error = Some(why),
                Verdict::Allowed(_) => {
                    this.configured = Some(hotkey);
                    this.restore(registrar);
                }
            },
        }
        this
    }

    pub fn configured(&self) -> Option<&Hotkey> {
        self.configured.as_ref()
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn is_recording(&self) -> bool {
        self.recording
    }

    /// Recording starts: the current hotkey is unregistered so that pressing it reaches
    /// the settings window instead of starting a lookup (spec 4.1).
    pub fn start_recording(&mut self, registrar: &mut impl Registrar) {
        if self.recording {
            return;
        }
        self.recording = true;
        if self.active {
            if let Some(hotkey) = &self.configured {
                registrar.unregister(hotkey);
            }
            self.active = false;
        }
    }

    /// Recording ends without a change (Esc, cancel, focus lost, window closed): the old
    /// hotkey comes back (spec 4.5).
    pub fn cancel_recording(&mut self, registrar: &mut impl Registrar) {
        if std::mem::take(&mut self.recording) {
            self.restore(registrar);
        }
    }

    /// Changes the hotkey to `new` (spec 5.1): register the new one first, save only
    /// once it works, and keep the old one whenever something fails. `save` writes the
    /// new canonical form to the settings file.
    pub fn apply(
        &mut self,
        registrar: &mut impl Registrar,
        new: Hotkey,
        confirmed: bool,
        save: impl FnOnce(&Hotkey) -> Result<(), String>,
    ) -> Applied {
        let warnings = match hotkey::check(&new) {
            Verdict::Blocked(why) => {
                self.finish_recording(registrar);
                return Applied::Blocked(why);
            }
            Verdict::Allowed(warnings) => warnings,
        };
        if self.configured.as_ref() == Some(&new) {
            self.finish_recording(registrar);
            return Applied::Unchanged;
        }
        if !warnings.is_empty() && !confirmed {
            return Applied::NeedsConfirmation(warnings);
        }
        if let Err(why) = registrar.register(&new) {
            self.finish_recording(registrar);
            return Applied::RegisterFailed(why);
        }
        if let Err(why) = save(&new) {
            registrar.unregister(&new);
            self.finish_recording(registrar);
            return Applied::SaveFailed(why);
        }
        if self.active
            && let Some(old) = &self.configured
        {
            registrar.unregister(old);
        }
        self.configured = Some(new);
        self.active = true;
        self.error = None;
        self.recording = false;
        Applied::Changed
    }

    fn finish_recording(&mut self, registrar: &mut impl Registrar) {
        self.recording = false;
        self.restore(registrar);
    }

    /// Registers `configured` again if it is not active. A failure (another program took
    /// it meanwhile) is kept in `error` (spec 5.1, item 7).
    fn restore(&mut self, registrar: &mut impl Registrar) {
        if self.active {
            return;
        }
        let Some(hotkey) = &self.configured else {
            return;
        };
        match registrar.register(hotkey) {
            Ok(()) => {
                self.active = true;
                self.error = None;
            }
            Err(why) => self.error = Some(why),
        }
    }

    /// The startup notice when the hotkey did not register, or `None` when it did.
    pub fn failure_message(&self) -> Option<String> {
        if self.active {
            return None;
        }
        let shown = self
            .configured
            .as_ref()
            .map(Hotkey::display)
            .map(|d| format!(" {d} "))
            .unwrap_or_default();
        let why = self.error.as_deref().unwrap_or("原因未知");
        Some(format!(
            "「{LOOKUP_LABEL}」的快捷键{shown}没有生效：{why}。\n\n\
             请右键托盘图标 → 设置… 更换快捷键，或关闭占用它的程序后重启 LexTrail。"
        ))
    }
}

/// The app's lookup hotkey, managed by Tauri.
pub struct HotkeyState(pub Mutex<LookupHotkey>);

/// [`Registrar`] backed by the global-shortcut plugin.
pub struct PluginRegistrar<'a, R: Runtime>(pub &'a AppHandle<R>);

impl<R: Runtime> Registrar for PluginRegistrar<'_, R> {
    fn register(&mut self, hotkey: &Hotkey) -> Result<(), String> {
        let shortcut = to_shortcut(hotkey)?;
        self.0
            .global_shortcut()
            .on_shortcut(shortcut, |app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    on_lookup(app);
                }
            })
            .map_err(|e| {
                eprintln!("hotkey {hotkey} did not register: {e}");
                "可能已被其他程序占用，或被系统保留".to_owned()
            })
    }

    fn unregister(&mut self, hotkey: &Hotkey) {
        if let Ok(shortcut) = to_shortcut(hotkey)
            && let Err(e) = self.0.global_shortcut().unregister(shortcut)
        {
            eprintln!("hotkey {hotkey} did not unregister: {e}");
        }
    }
}

fn to_shortcut(hotkey: &Hotkey) -> Result<Shortcut, String> {
    hotkey
        .to_global_shortcut()
        .parse::<Shortcut>()
        .map_err(|e| {
            eprintln!("hotkey {hotkey} has no plugin accelerator: {e}");
            "快捷键格式无效".to_owned()
        })
}

/// Registers the lookup hotkey from the settings and tells the user when it failed:
/// a native message box plus a tray tooltip (no WebView needed).
pub fn init<R: Runtime>(app: &AppHandle<R>, lookup: &str) {
    let state = LookupHotkey::startup(lookup, &mut PluginRegistrar(app));
    let message = state.failure_message();
    app.manage(HotkeyState(Mutex::new(state)));
    sync_tray(app);
    if let Some(message) = message {
        eprintln!("hotkey registration failed: {message}");
        crate::notice::warn(app, "LexTrail 快捷键注册失败", message);
    }
}

/// Runs `f` on the lookup hotkey state with the plugin registrar, then updates the tray
/// tooltip to match.
pub fn with_state<R: Runtime, T>(
    app: &AppHandle<R>,
    f: impl FnOnce(&mut LookupHotkey, &mut PluginRegistrar<'_, R>) -> T,
) -> T {
    let result = {
        let state = app.state::<HotkeyState>();
        let mut hotkey = state.0.lock().unwrap();
        f(&mut hotkey, &mut PluginRegistrar(app))
    };
    sync_tray(app);
    result
}

/// The tray tooltip says when the hotkey is not working; not while recording (turned off
/// on purpose).
fn sync_tray<R: Runtime>(app: &AppHandle<R>) {
    let failed = {
        let state = app.state::<HotkeyState>();
        let hotkey = state.0.lock().unwrap();
        !hotkey.is_active() && !hotkey.is_recording()
    };
    if let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID) {
        let tooltip = if failed {
            TRAY_TOOLTIP_FAILED
        } else {
            TRAY_TOOLTIP
        };
        let _ = tray.set_tooltip(Some(tooltip));
    }
}

fn on_lookup<R: Runtime>(app: &AppHandle<R>) {
    let pressed = Instant::now();
    // First, before anything else can happen (the popup, the capture): Alt is still held
    // now. Returns once the mask key is sent.
    crate::keyboard::mask_alt_release();
    #[cfg(debug_assertions)]
    eprintln!("hotkey triggered: lookup");
    if let Err(e) = crate::popup::lookup(app, pressed, crate::popup::Trigger::Hotkey) {
        eprintln!("cannot start lookup: {e}");
    }
}

#[cfg(test)]
mod tests {
    use lextrail_config::HotkeySettings;

    use super::*;

    /// Records registrations; combinations in `taken` belong to "another program".
    #[derive(Default)]
    struct FakeRegistrar {
        registered: Vec<String>,
        taken: Vec<String>,
        calls: Vec<String>,
    }

    impl FakeRegistrar {
        fn taking(taken: &[&str]) -> Self {
            Self {
                taken: taken.iter().map(|t| (*t).to_owned()).collect(),
                ..Self::default()
            }
        }
    }

    impl Registrar for FakeRegistrar {
        fn register(&mut self, hotkey: &Hotkey) -> Result<(), String> {
            let name = hotkey.canonical();
            self.calls.push(format!("+{name}"));
            if self.taken.contains(&name) || self.registered.contains(&name) {
                return Err("占用".into());
            }
            self.registered.push(name);
            Ok(())
        }

        fn unregister(&mut self, hotkey: &Hotkey) {
            let name = hotkey.canonical();
            self.calls.push(format!("-{name}"));
            self.registered.retain(|r| *r != name);
        }
    }

    fn hk(text: &str) -> Hotkey {
        Hotkey::parse(text).unwrap()
    }

    fn ok_save(_: &Hotkey) -> Result<(), String> {
        Ok(())
    }

    fn started(reg: &mut FakeRegistrar) -> LookupHotkey {
        let state = LookupHotkey::startup("Ctrl+Alt+X", reg);
        assert!(state.is_active());
        state
    }

    #[test]
    fn default_lookup_parses_and_is_ctrl_alt_x() {
        // D014: provisional default for lookup.
        let default = HotkeySettings::default().lookup;
        assert_eq!(hk(&default).canonical(), "Ctrl+Alt+X");
        assert!(
            hk(&default)
                .to_global_shortcut()
                .parse::<Shortcut>()
                .is_ok()
        );
    }

    #[test]
    fn every_key_converts_to_a_plugin_shortcut() {
        for text in [
            "Ctrl+Alt+X",
            "Ctrl+Shift+F24",
            "Alt+Shift+Minus",
            "Ctrl+Alt+Num5",
            "Ctrl+Alt+NumEnter",
            "Ctrl+Alt+NumAdd",
            "Ctrl+Alt+Up",
            "Pause",
            "Ctrl+Alt+Backquote",
            "Ctrl+Alt+Quote",
        ] {
            assert!(
                to_shortcut(&hk(text)).is_ok(),
                "{text} -> {}",
                hk(text).to_global_shortcut()
            );
        }
        // Same physical key as the plugin's own spelling.
        assert_eq!(
            to_shortcut(&hk("Ctrl+Alt+X")).unwrap(),
            "ctrl+alt+KeyX".parse::<Shortcut>().unwrap()
        );
    }

    #[test]
    fn startup_registers_the_settings_value() {
        let mut reg = FakeRegistrar::default();
        let state = started(&mut reg);
        assert_eq!(reg.registered, ["Ctrl+Alt+X"]);
        assert_eq!(state.failure_message(), None);
    }

    #[test]
    fn startup_failure_names_the_settings_window() {
        let mut reg = FakeRegistrar::taking(&["Ctrl+Alt+X"]);
        let state = LookupHotkey::startup("ctrl+alt+KeyX", &mut reg);
        assert!(!state.is_active());
        let message = state.failure_message().unwrap();
        assert!(message.contains("划词翻译"));
        assert!(message.contains("Ctrl + Alt + X"));
        assert!(message.contains("占用"));
        assert!(message.contains("右键托盘图标 → 设置"));
    }

    #[test]
    fn startup_rejects_invalid_and_blocked_values() {
        let mut reg = FakeRegistrar::default();
        let invalid = LookupHotkey::startup("Ctrl+Nope", &mut reg);
        assert!(invalid.failure_message().unwrap().contains("格式无效"));
        let blocked = LookupHotkey::startup("Ctrl+C", &mut reg);
        assert!(blocked.failure_message().unwrap().contains("连按两次"));
        assert!(reg.calls.is_empty());
    }

    #[test]
    fn recording_unregisters_and_cancel_restores() {
        let mut reg = FakeRegistrar::default();
        let mut state = started(&mut reg);
        state.start_recording(&mut reg);
        assert!(reg.registered.is_empty());
        assert!(state.is_recording() && !state.is_active());
        state.cancel_recording(&mut reg);
        assert_eq!(reg.registered, ["Ctrl+Alt+X"]);
        assert!(state.is_active() && !state.is_recording());
        // A second cancel (window destroyed after Esc) changes nothing.
        state.cancel_recording(&mut reg);
        assert_eq!(reg.calls, ["+Ctrl+Alt+X", "-Ctrl+Alt+X", "+Ctrl+Alt+X"]);
    }

    #[test]
    fn change_registers_new_then_saves() {
        let mut reg = FakeRegistrar::default();
        let mut state = started(&mut reg);
        state.start_recording(&mut reg);
        let mut saved = None;
        let applied = state.apply(&mut reg, hk("Ctrl+Shift+F1"), false, |h| {
            saved = Some(h.canonical());
            Ok(())
        });
        assert_eq!(applied, Applied::Changed);
        assert_eq!(saved.as_deref(), Some("Ctrl+Shift+F1"));
        assert_eq!(reg.registered, ["Ctrl+Shift+F1"]);
        assert_eq!(state.configured(), Some(&hk("Ctrl+Shift+F1")));
        assert!(state.is_active() && !state.is_recording());
    }

    #[test]
    fn change_without_recording_drops_the_old_one_last() {
        // "恢复默认" can be clicked without recording: the old one stays until the new
        // one works.
        let mut reg = FakeRegistrar::default();
        let mut state = started(&mut reg);
        state.apply(&mut reg, hk("F8"), false, ok_save);
        assert_eq!(reg.calls, ["+Ctrl+Alt+X", "+F8", "-Ctrl+Alt+X"]);
        assert_eq!(reg.registered, ["F8"]);
    }

    #[test]
    fn same_hotkey_is_unchanged_and_restored() {
        let mut reg = FakeRegistrar::default();
        let mut state = started(&mut reg);
        state.start_recording(&mut reg);
        let applied = state.apply(&mut reg, hk("Ctrl+Alt+X"), false, |_| {
            panic!("nothing to save")
        });
        assert_eq!(applied, Applied::Unchanged);
        assert_eq!(reg.registered, ["Ctrl+Alt+X"]);
        assert!(state.is_active() && !state.is_recording());
    }

    #[test]
    fn blocked_keeps_the_old_one() {
        let mut reg = FakeRegistrar::default();
        let mut state = started(&mut reg);
        state.start_recording(&mut reg);
        let applied = state.apply(&mut reg, hk("Ctrl+C"), true, |_| panic!("not saved"));
        assert!(matches!(applied, Applied::Blocked(_)));
        assert_eq!(reg.registered, ["Ctrl+Alt+X"]);
        assert!(state.is_active());
    }

    #[test]
    fn warnings_need_confirmation_first() {
        let mut reg = FakeRegistrar::default();
        let mut state = started(&mut reg);
        state.start_recording(&mut reg);
        let applied = state.apply(&mut reg, hk("Ctrl+V"), false, |_| panic!("not yet"));
        let Applied::NeedsConfirmation(warnings) = applied else {
            panic!("{applied:?}");
        };
        assert!(warnings[0].contains("粘贴"));
        // Still recording, nothing registered.
        assert!(state.is_recording());
        assert!(reg.registered.is_empty());
        assert_eq!(
            state.apply(&mut reg, hk("Ctrl+V"), true, ok_save),
            Applied::Changed
        );
        assert_eq!(reg.registered, ["Ctrl+V"]);
    }

    #[test]
    fn register_failure_keeps_the_old_one() {
        let mut reg = FakeRegistrar::taking(&["Ctrl+Alt+Q"]);
        let mut state = started(&mut reg);
        state.start_recording(&mut reg);
        let applied = state.apply(&mut reg, hk("Ctrl+Alt+Q"), false, |_| panic!("not saved"));
        assert_eq!(applied, Applied::RegisterFailed("占用".into()));
        assert_eq!(reg.registered, ["Ctrl+Alt+X"]);
        assert!(state.is_active() && state.error().is_none());
    }

    #[test]
    fn save_failure_drops_the_new_one() {
        let mut reg = FakeRegistrar::default();
        let mut state = started(&mut reg);
        state.start_recording(&mut reg);
        let applied = state.apply(&mut reg, hk("F8"), false, |_| Err("只读".into()));
        assert_eq!(applied, Applied::SaveFailed("只读".into()));
        assert_eq!(reg.registered, ["Ctrl+Alt+X"]);
        assert_eq!(state.configured(), Some(&hk("Ctrl+Alt+X")));
    }

    /// Another program takes the old hotkey while recording (spec 5.1, item 7).
    fn taken_while_recording() -> (FakeRegistrar, LookupHotkey) {
        let mut reg = FakeRegistrar::default();
        let mut state = started(&mut reg);
        state.start_recording(&mut reg);
        reg.taken.push("Ctrl+Alt+X".into());
        (reg, state)
    }

    #[test]
    fn restore_failure_after_cancel_is_reported() {
        let (mut reg, mut state) = taken_while_recording();
        state.cancel_recording(&mut reg);
        assert!(!state.is_active() && !state.is_recording());
        assert_eq!(state.error(), Some("占用"));
        assert!(state.failure_message().is_some());
    }

    #[test]
    fn restore_failure_after_register_failure_is_reported() {
        let (mut reg, mut state) = taken_while_recording();
        reg.taken.push("F8".into());
        let applied = state.apply(&mut reg, hk("F8"), false, |_| panic!("not saved"));
        assert!(matches!(applied, Applied::RegisterFailed(_)));
        assert!(!state.is_active());
        assert_eq!(state.error(), Some("占用"));
    }

    #[test]
    fn restore_failure_after_save_failure_is_reported() {
        let (mut reg, mut state) = taken_while_recording();
        let applied = state.apply(&mut reg, hk("F8"), false, |_| Err("只读".into()));
        assert!(matches!(applied, Applied::SaveFailed(_)));
        assert!(reg.registered.is_empty());
        assert!(!state.is_active());
        assert_eq!(state.error(), Some("占用"));
    }

    #[test]
    fn a_working_hotkey_after_a_startup_failure() {
        let mut reg = FakeRegistrar::taking(&["Ctrl+Alt+X"]);
        let mut state = LookupHotkey::startup("Ctrl+Alt+X", &mut reg);
        state.start_recording(&mut reg);
        state.cancel_recording(&mut reg);
        assert!(!state.is_active());
        assert_eq!(
            state.apply(&mut reg, hk("Ctrl+Shift+L"), false, ok_save),
            Applied::Changed
        );
        assert!(state.is_active() && state.error().is_none());
        assert_eq!(reg.registered, ["Ctrl+Shift+L"]);
    }

    #[test]
    fn a_working_hotkey_after_an_invalid_file_value() {
        let mut reg = FakeRegistrar::default();
        let mut state = LookupHotkey::startup("nonsense", &mut reg);
        assert_eq!(state.configured(), None);
        assert_eq!(
            state.apply(&mut reg, hk("Ctrl+Alt+Q"), false, ok_save),
            Applied::Changed
        );
        assert_eq!(reg.registered, ["Ctrl+Alt+Q"]);
    }
}
