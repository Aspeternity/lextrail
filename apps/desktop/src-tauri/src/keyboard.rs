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
//! sees both events; when the real key-up came first, a key-up is sent after the repeat.
//!
//! The hook's order is not always the order the key state is updated in: when two
//! processes inject keys at nearly the same moment (the repeat and a test script's
//! release, less than 0.2 ms apart), the hook can see the repeat first while the key
//! state ends up down (D014 supplement, seen in tools/e2e/stuck-keys.ps1). So once the
//! repeat came through, the hook keeps watching for a moment and the key state is read:
//! if the last real event of the key was its release and the key still reads down, a
//! key-up is sent too. An extra key-up of a key that is already up does no harm.

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

    /// The side-neutral virtual key (`VK_CONTROL`, `VK_SHIFT`). Programs that inject
    /// keys often send these (e.g. WScript's `SendKeys`), and hooks then report them so.
    pub fn generic_vk(self) -> u16 {
        match self {
            Self::LeftShift | Self::RightShift => 0x10,
            Self::LeftCtrl | Self::RightCtrl => 0x11,
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
    modifier: Modifier,
    vk: u32,
    generic_vk: u32,
    delivered: bool,
    released_first: bool,
    /// The latest real event of the key, before or after the repeat: `Some(true)` up.
    last_real_up: Option<bool>,
    /// The events seen, for the debug log (the first few only).
    seen: Vec<(u32, bool, bool)>,
}

impl RepeatWatch {
    const SEEN_LIMIT: usize = 16;

    pub fn new(modifier: Modifier) -> Self {
        Self {
            modifier,
            vk: u32::from(modifier.vk()),
            generic_vk: u32::from(modifier.generic_vk()),
            delivered: false,
            released_first: false,
            last_real_up: None,
            seen: Vec::new(),
        }
    }

    /// One key event as the hook saw it; `ours` = the injected repeat. Events of the
    /// side-neutral key count too: a release of `VK_CONTROL` also releases left Ctrl.
    pub fn on_key(&mut self, vk: u32, up: bool, ours: bool) {
        if self.seen.len() < Self::SEEN_LIMIT {
            self.seen.push((vk, up, ours));
        }
        if vk != self.vk && vk != self.generic_vk {
            return;
        }
        if !ours {
            self.last_real_up = Some(up);
        }
        if self.delivered {
            return;
        }
        if ours {
            self.delivered |= !up;
        } else {
            // Released, or released and pressed again: only the latest real event counts.
            self.released_first = up;
        }
    }

    pub fn modifier(&self) -> Modifier {
        self.modifier
    }

    pub fn delivered(&self) -> bool {
        self.delivered
    }

    /// The repeat came after the real key-up, so the key is logically stuck down.
    pub fn needs_release(&self) -> bool {
        self.delivered && self.released_first
    }

    /// What to do once the repeat came through and the hook watched a little longer;
    /// `key_down` reads the modifier's key state (`GetAsyncKeyState`).
    pub fn decide(&self, key_down: impl FnOnce(Modifier) -> bool) -> Release {
        if !self.delivered {
            Release::NotDelivered
        } else if self.needs_release() {
            Release::ReleasedFirst
        } else if self.last_real_up == Some(true) && key_down(self.modifier) {
            // The hook saw the release after the repeat, yet the key reads down: the
            // key state took the two in the other order.
            Release::StillDown
        } else {
            Release::None
        }
    }

    /// The events seen, e.g. `A2v* 11^ A4^` (virtual key in hex, `v` down, `^` up,
    /// `*` ours).
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn seen(&self) -> String {
        self.seen
            .iter()
            .map(|&(vk, up, ours)| {
                format!(
                    "{vk:X}{}{}",
                    if up { "^" } else { "v" },
                    if ours { "*" } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The outcome of a watched repeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Release {
    /// The repeat never came through the hook; nothing is sent.
    NotDelivered,
    /// Still held, or released normally afterwards: nothing to do.
    None,
    /// The hook saw the real key-up before the repeat: send a key-up.
    ReleasedFirst,
    /// The hook saw the real key-up after the repeat, but the key state still reads
    /// down: send a key-up.
    StillDown,
}

impl Release {
    pub fn send_key_up(self) -> bool {
        matches!(self, Self::ReleasedFirst | Self::StillDown)
    }
}

/// Call first thing when a hotkey fires, while the hotkey's keys are still held. Returns
/// once the mask key has been sent (or found unnecessary, a few milliseconds); the
/// stuck-key check then finishes on the hook's thread.
#[cfg(windows)]
pub fn mask_alt_release() {
    win::mask_alt_release();
}

#[cfg(not(windows))]
pub fn mask_alt_release() {}

#[cfg(windows)]
mod win {
    use std::mem::size_of;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread;
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
        KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput,
        VIRTUAL_KEY, VK_LCONTROL, VK_LSHIFT, VK_MENU, VK_RCONTROL, VK_RSHIFT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLKHF_UP,
        MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, PostThreadMessageW, QS_ALLINPUT,
        SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_NULL,
    };

    use super::{HeldKeys, Mask, Modifier, Release, RepeatWatch, choose_mask};

    /// Virtual key 0xE8: unassigned, so applications have no meaning for it.
    const UNASSIGNED_KEY: u16 = 0xE8;
    /// Marks our injected events (`dwExtraInfo`), so the hook can tell them apart.
    const TAG: usize = 0x4C58_4D4B;
    /// How long the hotkey thread waits for the hook to be in place.
    const HOOK_WAIT: Duration = Duration::from_millis(50);
    /// How long the hook waits for the repeat to come through.
    const DELIVERY_WAIT: Duration = Duration::from_millis(200);
    /// After installing the hook and before reading the keys: a key event that passed
    /// the hook chain just before the hook was installed may not have reached the key
    /// state yet, and would be missed by both.
    const SETTLE: Duration = Duration::from_millis(2);
    /// After the repeat came through, how long the hook keeps watching before the key
    /// state is read: a release injected at the same moment reaches the hook and the key
    /// state within it.
    const CHECK_DELAY: Duration = Duration::from_millis(2);

    /// The repeat being watched by the hook; one at a time.
    static WATCH: Mutex<Option<RepeatWatch>> = Mutex::new(None);
    /// Real key events the hook saw before a repeat was armed (the keys are read in
    /// between): a release right after the read must count too, or the repeat would
    /// leave the key stuck down (seen in tools/e2e/popup-e2e.ps1: hook saw `A4^ A2v*`).
    static EARLY: Mutex<Vec<(u32, bool)>> = Mutex::new(Vec::new());
    const EARLY_LIMIT: usize = 64;

    fn early() -> std::sync::MutexGuard<'static, Vec<(u32, bool)>> {
        EARLY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn watch() -> std::sync::MutexGuard<'static, Option<RepeatWatch>> {
        WATCH
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
        // The hook goes in before the keys are read, so that a release right after the
        // read is seen by it. It runs on a thread of its own, and the repeat is sent from
        // this thread: a thread's own SendInput reaches its own hook out of order (ahead
        // of real events already queued), which would hide exactly the race the hook is
        // for (seen in tools/e2e/stuck-keys.ps1).
        let watcher = Watcher::start();
        let mask = choose_mask(held_keys());
        match (mask, &watcher) {
            (None, _) => {}
            (Some(Mask::Repeat(modifier)), Some(w)) if w.arm(modifier) => {
                send(&[key(modifier.vk(), modifier.extended(), false)]);
            }
            // Without the hook a stuck key would go unnoticed: use the plain key-press.
            (Some(_), _) => {
                send_unassigned();
                #[cfg(debug_assertions)]
                eprintln!("hotkey: alt mask: 0xE8");
            }
        }
        if let Some(watcher) = watcher {
            watcher.done();
        }
    }

    /// The hook thread, from the hotkey thread's side.
    struct Watcher {
        thread_id: u32,
        flags: Arc<Flags>,
    }

    #[derive(Default)]
    struct Flags {
        /// This watcher's repeat is in `WATCH`.
        armed: AtomicBool,
        /// The hotkey thread has sent the mask (or decided against it).
        done: AtomicBool,
    }

    impl Watcher {
        /// Installs the hook on a new thread; `None` if that failed or took too long.
        fn start() -> Option<Self> {
            let (ready, wait) = mpsc::channel();
            let flags = Arc::new(Flags::default());
            let thread_flags = Arc::clone(&flags);
            let spawned = thread::Builder::new()
                .name("hotkey-mask".into())
                .spawn(move || hook_thread(&ready, &thread_flags));
            if let Err(e) = spawned {
                eprintln!("hotkey: could not start the Alt mask thread: {e}");
                return None;
            }
            match wait.recv_timeout(HOOK_WAIT) {
                Ok(Some(thread_id)) => Some(Self { thread_id, flags }),
                Ok(None) => None,
                Err(_) => {
                    // Gave up on it; it removes its hook once it sees `done`.
                    flags.done.store(true, Ordering::SeqCst);
                    eprintln!("hotkey: the Alt mask hook took too long; using 0xE8");
                    None
                }
            }
        }

        /// Starts watching `modifier`; false if another hotkey's repeat is still watched.
        fn arm(&self, modifier: Modifier) -> bool {
            let mut w = watch();
            if w.is_some() {
                return false;
            }
            let mut repeat = RepeatWatch::new(modifier);
            // Everything real since the hook went in, in order: only the latest event of
            // the key counts, so a release (and a press again) before the read is harmless.
            for (vk, up) in early().drain(..) {
                repeat.on_key(vk, up, false);
            }
            *w = Some(repeat);
            self.flags.armed.store(true, Ordering::SeqCst);
            true
        }

        /// The mask is out (or not needed): the hook thread finishes and wakes up for it.
        fn done(self) {
            self.flags.done.store(true, Ordering::SeqCst);
            // SAFETY: plain Win32 call; WM_NULL does nothing but wake the thread.
            let _ = unsafe { PostThreadMessageW(self.thread_id, WM_NULL, WPARAM(0), LPARAM(0)) };
        }
    }

    /// Installs the hook, reports the thread id (`None` on failure), then pumps messages
    /// until the hotkey thread is done and the repeat, if one was sent, came through.
    fn hook_thread(ready: &mpsc::Sender<Option<u32>>, flags: &Flags) {
        early().clear();
        // SAFETY: plain Win32 calls; the hook procedure only touches `WATCH`, and is
        // removed below on this thread.
        let hook = unsafe {
            GetModuleHandleW(None).and_then(|module| {
                SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(hook_proc),
                    Some(HINSTANCE(module.0)),
                    0,
                )
            })
        };
        let hook = match hook {
            Ok(hook) => hook,
            Err(e) => {
                eprintln!("hotkey: no keyboard hook for the Alt mask ({e}); using 0xE8");
                let _ = ready.send(None);
                return;
            }
        };
        // Spinning: a blocking wait this short lasts a whole timer tick (~15 ms).
        pump_until(Instant::now() + SETTLE, Wait::Spin, || false);
        // SAFETY: plain Win32 call.
        let _ = ready.send(Some(unsafe { GetCurrentThreadId() }));

        let start = Instant::now();
        pump_until(start + DELIVERY_WAIT, Wait::Block, || {
            flags.done.load(Ordering::SeqCst)
                && (!flags.armed.load(Ordering::SeqCst)
                    || watch().as_ref().is_none_or(RepeatWatch::delivered))
        });
        let armed = flags.armed.load(Ordering::SeqCst);
        if armed && watch().as_ref().is_some_and(RepeatWatch::delivered) {
            pump_until(Instant::now() + CHECK_DELAY, Wait::Spin, || false);
        }
        // Only this watcher's own repeat (another hotkey's may be in `WATCH`).
        let finished = if armed { watch().take() } else { None };
        // SAFETY: the hook was installed on this thread.
        if let Err(e) = unsafe { UnhookWindowsHookEx(hook) } {
            eprintln!("hotkey: could not remove the Alt mask hook: {e}");
        }
        if let Some(w) = finished {
            let summary = finish_repeat(&w);
            #[cfg(debug_assertions)]
            eprintln!(
                "hotkey: alt mask: {summary} ({:.1}ms; hook saw {})",
                start.elapsed().as_secs_f64() * 1000.0,
                w.seen()
            );
            #[cfg(not(debug_assertions))]
            let _ = summary;
        }
    }

    /// Releases the key if the user had released it before the repeat, or if it reads
    /// down although the hook saw its release.
    fn finish_repeat(w: &RepeatWatch) -> String {
        let modifier = w.modifier();
        let release = w.decide(|m| {
            // SAFETY: plain Win32 call. The high bit is set while the key is down.
            let state = unsafe { GetAsyncKeyState(i32::from(m.vk())) };
            state < 0
        });
        if release.send_key_up() {
            // Tagged like the repeat, so our hook ignores it.
            send(&[key(modifier.vk(), modifier.extended(), true)]);
        }
        // Worth knowing in release builds too: it means the race happened.
        match release {
            Release::ReleasedFirst => {
                eprintln!("hotkey: {modifier:?} was released before its mask repeat; released it");
                format!("repeat {modifier:?}, released first, key-up sent")
            }
            Release::StillDown => {
                eprintln!(
                    "hotkey: {modifier:?} still down after its release was seen; released it"
                );
                format!("repeat {modifier:?}, still down after release, key-up sent")
            }
            Release::None => format!("repeat {modifier:?}"),
            Release::NotDelivered => {
                eprintln!("hotkey: the {modifier:?} mask repeat did not come through the hook");
                format!("repeat {modifier:?}, not seen by the hook")
            }
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Wait {
        /// Sleep until a message arrives (hook calls and our wake-up are messages) or
        /// the deadline.
        Block,
        /// Poll without sleeping, for waits shorter than a timer tick.
        Spin,
    }

    /// Pumps this thread's messages (low-level hooks run inside them) until `done` or
    /// `deadline`.
    fn pump_until(deadline: Instant, wait: Wait, done: impl Fn() -> bool) {
        loop {
            if done() {
                return;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            // SAFETY: plain Win32 calls on this thread's message queue.
            unsafe {
                if wait == Wait::Block {
                    let millis = left.as_millis().max(1) as u32;
                    let _ = MsgWaitForMultipleObjects(None, false, millis, QS_ALLINPUT);
                } else {
                    thread::yield_now();
                }
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    DispatchMessageW(&msg);
                }
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
            if let Some(w) = watch().as_mut() {
                w.on_key(event.vkCode, up, ours);
            } else if !injected {
                let mut early = early();
                if early.len() < EARLY_LIMIT {
                    early.push((event.vkCode, up));
                }
            }
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
    fn release_of_the_side_neutral_key_counts() {
        // WScript's SendKeys releases VK_CONTROL, not VK_LCONTROL.
        let mut w = RepeatWatch::new(Modifier::LeftCtrl);
        w.on_key(0x11, true, false);
        w.on_key(LCTRL, false, true);
        assert!(w.needs_release());
        assert_eq!(w.seen(), "11^ A2v*");
    }

    #[test]
    fn undelivered_repeat_is_never_released() {
        let mut w = RepeatWatch::new(Modifier::LeftCtrl);
        w.on_key(LCTRL, true, false);
        assert!(!w.delivered());
        assert!(!w.needs_release());
    }

    const LSHIFT: u32 = 0xA0;
    const REPEATED: [(Modifier, u32); 2] =
        [(Modifier::LeftCtrl, LCTRL), (Modifier::LeftShift, LSHIFT)];

    /// `decide` with a key state that reads `down`; checks that the repeated key is read.
    fn decide_with(w: &RepeatWatch, down: bool) -> Release {
        let expected = w.modifier();
        w.decide(|m| {
            assert_eq!(m, expected);
            down
        })
    }

    #[test]
    fn release_seen_after_the_repeat_but_key_still_down_is_released() {
        // Hook order: the repeat, then the real key-up; the key state took them the
        // other way round (stuck-keys.ps1, D014 supplement).
        for (modifier, vk) in REPEATED {
            let mut w = RepeatWatch::new(modifier);
            w.on_key(vk, false, true);
            w.on_key(vk, true, false);
            assert!(!w.needs_release());
            let release = decide_with(&w, true);
            assert_eq!(release, Release::StillDown, "{modifier:?}");
            assert!(release.send_key_up());
        }
    }

    #[test]
    fn release_seen_after_the_repeat_with_the_key_up_sends_nothing() {
        for (modifier, vk) in REPEATED {
            let mut w = RepeatWatch::new(modifier);
            w.on_key(vk, false, true);
            w.on_key(vk, true, false);
            let release = decide_with(&w, false);
            assert_eq!(release, Release::None, "{modifier:?}");
            assert!(!release.send_key_up());
        }
    }

    #[test]
    fn pressed_again_before_the_check_sends_nothing() {
        for (modifier, vk) in REPEATED {
            let mut w = RepeatWatch::new(modifier);
            w.on_key(vk, false, true);
            w.on_key(vk, true, false);
            w.on_key(vk, false, false);
            assert_eq!(decide_with(&w, true), Release::None, "{modifier:?}");
        }
    }

    #[test]
    fn still_held_sends_nothing() {
        for (modifier, vk) in REPEATED {
            let mut w = RepeatWatch::new(modifier);
            w.on_key(vk, false, true);
            assert_eq!(decide_with(&w, true), Release::None, "{modifier:?}");
        }
    }

    #[test]
    fn released_first_sends_a_key_up_whatever_the_key_state() {
        let mut w = RepeatWatch::new(Modifier::LeftCtrl);
        w.on_key(0x11, true, false);
        w.on_key(LCTRL, false, true);
        let release = w.decide(|_| false);
        assert_eq!(release, Release::ReleasedFirst);
        assert!(release.send_key_up());
    }

    #[test]
    fn undelivered_repeat_sends_nothing() {
        let mut w = RepeatWatch::new(Modifier::LeftShift);
        w.on_key(LSHIFT, true, false);
        let release = w.decide(|_| true);
        assert_eq!(release, Release::NotDelivered);
        assert!(!release.send_key_up());
    }
}
