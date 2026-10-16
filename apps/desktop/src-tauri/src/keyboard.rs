//! Keeps a global hotkey from looking like a lone Alt press to the application behind it.
//!
//! The hotkey's letter key-down is consumed by the system, so the foreground application
//! sees Alt go down and up with no other key pressed in between (only the letter's key-up).
//! Many applications treat that as "Alt pressed on its own" and focus their menu bar
//! (VS Code, R1-1). So while Alt is still held, one more key-down is sent and the Alt
//! release is no longer a lone one (D014):
//!
//! - **A modifier of the hotkey that is still held** (Ctrl, else Shift) gets one more
//!   key-down, like an auto-repeat, and no key-up: its logical state stays what the
//!   keyboard says, and the user's own release ends it. A modifier, unlike an ordinary
//!   key, does not make Windows Terminal drop its selection (R3-P1).
//! - **Only Alt is held** (e.g. an Alt+letter hotkey): the unassigned virtual key 0xE8 is
//!   pressed and released, as AutoHotkey does with its menu mask key. Windows Terminal
//!   drops its selection on it (known limitation, D014).
//!
//! The repeat has one race: the user may release the modifier between the check and the
//! injected key-down, which then arrives after the real key-up and leaves the modifier
//! logically down. A low-level keyboard hook, installed only for these few milliseconds,
//! sees both events in the order the system processes them; when the real key-up came
//! first, a key-up is sent after the repeat.

/// Keys held when the hotkey fired.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeldKeys {
    pub alt: bool,
    pub left_ctrl: bool,
    pub right_ctrl: bool,
    pub left_shift: bool,
    pub right_shift: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    LeftCtrl,
    RightCtrl,
    LeftShift,
    RightShift,
}

impl Modifier {
    /// The side-specific virtual key, as low-level hooks report it.
    pub fn vk(self) -> u16 {
        match self {
            Self::LeftShift => 0xA0,
            Self::RightShift => 0xA1,
            Self::LeftCtrl => 0xA2,
            Self::RightCtrl => 0xA3,
        }
    }

    /// Right Ctrl is an extended key; the others are not.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn extended(self) -> bool {
        self == Self::RightCtrl
    }
}

/// What to send so that the Alt release is not a lone one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mask {
    /// One more key-down of a held modifier.
    Repeat(Modifier),
    /// Press and release the unassigned virtual key 0xE8.
    Unassigned,
}

/// `None` when Alt is not held: its release has already happened, or it is not part of
/// the hotkey. Ctrl is preferred over Shift (a Shift tap with Alt held could switch the
/// input language, so Shift only ever gets a key-down too).
pub fn choose_mask(held: HeldKeys) -> Option<Mask> {
    if !held.alt {
        return None;
    }
    let modifier = [
        (held.left_ctrl, Modifier::LeftCtrl),
        (held.right_ctrl, Modifier::RightCtrl),
        (held.left_shift, Modifier::LeftShift),
        (held.right_shift, Modifier::RightShift),
    ]
    .into_iter()
    .find_map(|(down, m)| down.then_some(m));
    Some(modifier.map_or(Mask::Unassigned, Mask::Repeat))
}

/// Follows the low-level hook's view of the repeated modifier until the repeat itself
/// comes through, to tell whether the user released the key just before it.
#[derive(Debug)]
pub struct RepeatWatch {
    vk: u32,
    delivered: bool,
    released_first: bool,
}

impl RepeatWatch {
    pub fn new(modifier: Modifier) -> Self {
        Self {
            vk: u32::from(modifier.vk()),
            delivered: false,
            released_first: false,
        }
    }

    /// One key event as the hook saw it; `ours` = the injected repeat.
    pub fn on_key(&mut self, vk: u32, up: bool, ours: bool) {
        if self.delivered || vk != self.vk {
            return;
        }
        if ours {
            self.delivered |= !up;
        } else {
            // Released, or released and pressed again: only the latest real event counts.
            self.released_first = up;
        }
    }

    pub fn delivered(&self) -> bool {
        self.delivered
    }

    /// The repeat came after the real key-up, so the key is logically stuck down.
    pub fn needs_release(&self) -> bool {
        self.delivered && self.released_first
    }
}

/// Call first thing when a hotkey fires, while the hotkey's keys are still held. Returns
/// once the mask key has been sent (or found unnecessary); the stuck-key check then
/// finishes on its own thread.
#[cfg(windows)]
pub fn mask_alt_release() {
    win::mask_alt_release();
}

#[cfg(not(windows))]
pub fn mask_alt_release() {}

#[cfg(windows)]
mod win {
    use std::cell::RefCell;
    use std::mem::size_of;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
        KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput,
        VIRTUAL_KEY, VK_LCONTROL, VK_LSHIFT, VK_MENU, VK_RCONTROL, VK_RSHIFT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLKHF_UP,
        MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT, SetWindowsHookExW,
        UnhookWindowsHookEx, WH_KEYBOARD_LL,
    };

    use super::{HeldKeys, Mask, Modifier, RepeatWatch, choose_mask};

    /// Virtual key 0xE8: unassigned, so applications have no meaning for it.
    const UNASSIGNED_KEY: u16 = 0xE8;
    /// Marks our injected events (`dwExtraInfo`), so the hook can tell them apart.
    const TAG: usize = 0x4C58_4D4B;
    /// How long the hotkey thread waits for the mask to be sent.
    const SEND_WAIT: Duration = Duration::from_millis(50);
    /// How long the hook waits for the repeat to come through.
    const DELIVERY_WAIT: Duration = Duration::from_millis(200);

    thread_local! {
        /// The watch of the hook installed on this thread.
        static WATCH: RefCell<Option<RepeatWatch>> = const { RefCell::new(None) };
    }

    pub fn mask_alt_release() {
        // Debug builds only: LEXTRAIL_DEBUG_NO_ALT_MASK=1 turns the mask off, to reproduce
        // the lone-Alt problem; LEXTRAIL_DEBUG_ALT_MASK_E8=1 always sends 0xE8 (the
        // behaviour before R3-P1), to compare.
        #[cfg(debug_assertions)]
        if std::env::var_os("LEXTRAIL_DEBUG_NO_ALT_MASK").is_some() {
            return;
        }
        #[cfg(debug_assertions)]
        if std::env::var_os("LEXTRAIL_DEBUG_ALT_MASK_E8").is_some() {
            if held_keys().alt {
                send_unassigned();
                eprintln!("hotkey: alt mask: 0xE8 (forced)");
            }
            return;
        }
        // Alt already released: nothing to mask, and no hook needed.
        if !held_keys().alt {
            return;
        }
        let (sent, wait) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("hotkey-mask".into())
            .spawn(move || {
                mask_on_this_thread(&|| {
                    let _ = sent.send(());
                })
            });
        match spawned {
            // Whatever the lookup does next comes after the mask.
            Ok(_) => {
                let _ = wait.recv_timeout(SEND_WAIT);
            }
            Err(e) => eprintln!("hotkey: could not start the Alt mask thread: {e}"),
        }
    }

    fn mask_on_this_thread(sent: &dyn Fn()) {
        // The hook goes in before the keys are read, so that a release right after the
        // read is seen by it.
        let hook = unsafe {
            // SAFETY: plain Win32 calls; the hook procedure only touches this thread's
            // `WATCH` and is removed below on this thread.
            GetModuleHandleW(None).and_then(|module| {
                SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(hook_proc),
                    Some(HINSTANCE(module.0)),
                    0,
                )
            })
        };
        let mask = choose_mask(held_keys());
        let result = match (mask, &hook) {
            (None, _) => None,
            (Some(Mask::Repeat(modifier)), Ok(_)) => {
                WATCH.with_borrow_mut(|w| *w = Some(RepeatWatch::new(modifier)));
                send(&[key(modifier.vk(), modifier.extended(), false)]);
                // The hotkey thread can go on; the stuck-key check needs no waiting there.
                sent();
                Some(finish_repeat(modifier))
            }
            // Without the hook a stuck key would go unnoticed: use the plain key-press.
            (Some(Mask::Repeat(_) | Mask::Unassigned), hook) => {
                if let Err(e) = hook {
                    eprintln!("hotkey: no keyboard hook for the Alt mask ({e}); using 0xE8");
                }
                send_unassigned();
                Some("0xE8".to_owned())
            }
        };
        // Also for the paths above that sent nothing or only 0xE8 (a second call is harmless).
        sent();
        if let Ok(hook) = hook {
            // SAFETY: the hook was installed on this thread.
            if let Err(e) = unsafe { UnhookWindowsHookEx(hook) } {
                eprintln!("hotkey: could not remove the Alt mask hook: {e}");
            }
        }
        #[cfg(debug_assertions)]
        if let Some(result) = result {
            eprintln!("hotkey: alt mask: {result}");
        }
        #[cfg(not(debug_assertions))]
        let _ = result;
    }

    /// Pumps messages (the hook runs inside them) until the repeat has come through, then
    /// releases the key if the user had released it first.
    fn finish_repeat(modifier: Modifier) -> String {
        let start = Instant::now();
        let deadline = start + DELIVERY_WAIT;
        loop {
            if WATCH.with_borrow(|w| w.as_ref().is_some_and(RepeatWatch::delivered)) {
                break;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            // SAFETY: plain Win32 calls on this thread's message queue.
            unsafe {
                let _ =
                    MsgWaitForMultipleObjects(None, false, left.as_millis() as u32, QS_ALLINPUT);
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    DispatchMessageW(&msg);
                }
            }
        }
        let watch = WATCH.with_borrow_mut(Option::take);
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        match watch {
            Some(w) if w.needs_release() => {
                send(&[key(modifier.vk(), modifier.extended(), true)]);
                // Worth knowing in release builds too: it means the race happened.
                eprintln!("hotkey: {modifier:?} was released before its mask repeat; released it");
                format!("repeat {modifier:?}, released first, key-up sent ({elapsed:.1}ms)")
            }
            Some(w) if w.delivered() => format!("repeat {modifier:?} ({elapsed:.1}ms)"),
            _ => {
                eprintln!("hotkey: the {modifier:?} mask repeat did not come through the hook");
                format!("repeat {modifier:?}, not seen by the hook")
            }
        }
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            // SAFETY: for HC_ACTION, `lparam` points to a KBDLLHOOKSTRUCT.
            let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            let injected = event.flags.0 & LLKHF_INJECTED.0 != 0;
            let up = event.flags.0 & LLKHF_UP.0 != 0;
            let ours = injected && event.dwExtraInfo == TAG;
            WATCH.with_borrow_mut(|w| {
                if let Some(w) = w {
                    w.on_key(event.vkCode, up, ours);
                }
            });
        }
        // SAFETY: passes the event on unchanged.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    fn held_keys() -> HeldKeys {
        // SAFETY: plain Win32 call. The high bit is set while the key is down.
        let down = |vk: VIRTUAL_KEY| unsafe { GetAsyncKeyState(i32::from(vk.0)) } < 0;
        HeldKeys {
            alt: down(VK_MENU),
            left_ctrl: down(VK_LCONTROL),
            right_ctrl: down(VK_RCONTROL),
            left_shift: down(VK_LSHIFT),
            right_shift: down(VK_RSHIFT),
        }
    }

    fn key(vk: u16, extended: bool, up: bool) -> INPUT {
        let mut flags = KEYBD_EVENT_FLAGS(0);
        if extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if up {
            flags |= KEYEVENTF_KEYUP;
        }
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    // With the scan code, like a real keyboard (browsers report `code`).
                    // SAFETY: plain Win32 call.
                    wScan: unsafe { MapVirtualKeyW(u32::from(vk), MAPVK_VK_TO_VSC) } as u16,
                    dwFlags: flags,
                    dwExtraInfo: TAG,
                    ..Default::default()
                },
            },
        }
    }

    fn send_unassigned() {
        send(&[
            key(UNASSIGNED_KEY, false, false),
            key(UNASSIGNED_KEY, false, true),
        ]);
    }

    fn send(inputs: &[INPUT]) {
        // SAFETY: `inputs` is a valid slice of keyboard INPUTs.
        let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
        if sent as usize != inputs.len() {
            eprintln!(
                "hotkey: could not send the Alt mask ({sent} of {} events)",
                inputs.len()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(f: impl FnOnce(&mut HeldKeys)) -> HeldKeys {
        let mut keys = HeldKeys {
            alt: true,
            ..HeldKeys::default()
        };
        f(&mut keys);
        keys
    }

    #[test]
    fn no_mask_once_alt_is_released() {
        let keys = HeldKeys {
            left_ctrl: true,
            ..HeldKeys::default()
        };
        assert_eq!(choose_mask(keys), None);
    }

    #[test]
    fn repeats_ctrl_for_the_default_hotkey() {
        // Ctrl+Alt+X with either Ctrl key.
        assert_eq!(
            choose_mask(held(|k| k.left_ctrl = true)),
            Some(Mask::Repeat(Modifier::LeftCtrl))
        );
        assert_eq!(
            choose_mask(held(|k| k.right_ctrl = true)),
            Some(Mask::Repeat(Modifier::RightCtrl))
        );
    }

    #[test]
    fn prefers_ctrl_over_shift_and_falls_back_to_shift() {
        assert_eq!(
            choose_mask(held(|k| {
                k.left_shift = true;
                k.left_ctrl = true;
            })),
            Some(Mask::Repeat(Modifier::LeftCtrl))
        );
        assert_eq!(
            choose_mask(held(|k| k.right_shift = true)),
            Some(Mask::Repeat(Modifier::RightShift))
        );
    }

    #[test]
    fn alt_alone_uses_the_unassigned_key() {
        assert_eq!(choose_mask(held(|_| {})), Some(Mask::Unassigned));
    }

    const LCTRL: u32 = 0xA2;

    #[test]
    fn repeat_while_held_needs_no_release() {
        let mut w = RepeatWatch::new(Modifier::LeftCtrl);
        w.on_key(LCTRL, false, true);
        assert!(w.delivered());
        assert!(!w.needs_release());
        // The user's release afterwards is the normal end of the key.
        w.on_key(LCTRL, true, false);
        assert!(!w.needs_release());
    }

    #[test]
    fn release_before_the_repeat_needs_a_key_up() {
        let mut w = RepeatWatch::new(Modifier::LeftCtrl);
        w.on_key(LCTRL, true, false);
        assert!(!w.needs_release(), "not decided before the repeat arrives");
        w.on_key(LCTRL, false, true);
        assert!(w.needs_release());
    }

    #[test]
    fn released_and_pressed_again_before_the_repeat_is_still_held() {
        let mut w = RepeatWatch::new(Modifier::LeftCtrl);
        w.on_key(LCTRL, true, false);
        w.on_key(LCTRL, false, false);
        w.on_key(LCTRL, false, true);
        assert!(!w.needs_release());
    }

    #[test]
    fn other_keys_and_injected_events_of_others_are_ignored() {
        let mut w = RepeatWatch::new(Modifier::LeftCtrl);
        // Right Ctrl and Alt going up do not matter.
        w.on_key(0xA3, true, false);
        w.on_key(0xA4, true, false);
        w.on_key(LCTRL, false, true);
        assert!(w.delivered());
        assert!(!w.needs_release());
    }

    #[test]
    fn undelivered_repeat_is_never_released() {
        let mut w = RepeatWatch::new(Modifier::LeftCtrl);
        w.on_key(LCTRL, true, false);
        assert!(!w.delivered());
        assert!(!w.needs_release());
    }
}
