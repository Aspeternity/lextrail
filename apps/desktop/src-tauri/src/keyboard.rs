//! Keeps a global hotkey from looking like a lone Alt press to the application behind it.
//!
//! The hotkey's letter key-down is consumed by the system, so the foreground application
//! sees Alt go down and up with no other key pressed in between (only the letter's key-up).
//! Many applications treat that as "Alt pressed on its own" and focus their menu bar
//! (VS Code, R1-1). As AutoHotkey does with its menu mask key, a key press of the
//! unassigned virtual key 0xE8 is sent while Alt is still held, so the Alt release is no
//! longer a lone one (D014).

/// Virtual key 0xE8: unassigned, so applications have no meaning for it.
#[cfg(windows)]
const MASK_KEY: u16 = 0xE8;

/// Call when a hotkey fires. Does nothing unless Alt is held right now.
#[cfg(windows)]
pub fn mask_alt_release() {
    use std::mem::size_of;

    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
        KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_MENU,
    };

    // Debug builds only: LEXTRAIL_DEBUG_NO_ALT_MASK=1 turns this off, to reproduce the
    // lone-Alt problem when testing.
    #[cfg(debug_assertions)]
    if std::env::var_os("LEXTRAIL_DEBUG_NO_ALT_MASK").is_some() {
        return;
    }
    // SAFETY: plain Win32 call. The high bit is set while the key is down.
    if unsafe { GetAsyncKeyState(i32::from(VK_MENU.0)) } >= 0 {
        return;
    }
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(MASK_KEY),
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    // SAFETY: `inputs` is a valid slice of keyboard INPUTs.
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        eprintln!("hotkey: could not send the Alt mask key ({sent} of 2 events)");
    }
}

#[cfg(not(windows))]
pub fn mask_alt_release() {}
