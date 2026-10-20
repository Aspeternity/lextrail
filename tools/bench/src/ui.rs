//! Simulated input and popup window observation.

#![cfg(windows)]

use std::mem::size_of;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_CONTROL, VK_ESCAPE, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetForegroundWindow, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
    PostMessageW, SetCursorPos, WM_CLOSE,
};
use windows::core::w;

const VK_X: VIRTUAL_KEY = VIRTUAL_KEY(0x58);

fn key(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

/// Presses and releases `keys` as one chord, in a single `SendInput` call.
fn chord(keys: &[VIRTUAL_KEY]) -> Result<()> {
    let mut inputs: Vec<INPUT> = keys.iter().map(|&k| key(k, KEYBD_EVENT_FLAGS(0))).collect();
    inputs.extend(keys.iter().rev().map(|&k| key(k, KEYEVENTF_KEYUP)));
    // SAFETY: `inputs` is a valid slice of keyboard INPUTs.
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        bail!("SendInput delivered {sent} of {} events", inputs.len());
    }
    Ok(())
}

/// The default lookup hotkey (D014).
pub fn press_hotkey() -> Result<()> {
    chord(&[VK_CONTROL, VK_MENU, VK_X])
}

pub fn press_escape() -> Result<()> {
    chord(&[VK_ESCAPE])
}

/// Puts the cursor at a fixed spot so every sample shows the popup on the same monitor.
pub fn park_cursor() -> Result<()> {
    // SAFETY: plain Win32 call.
    unsafe { SetCursorPos(600, 400) }?;
    Ok(())
}

/// The app's popup window ("Tauri Window" titled "LexTrail"), if it exists.
pub fn find_popup(pid: u32) -> Option<HWND> {
    find_window(pid, w!("LexTrail"))
}

/// The app's settings window (D047), if it exists.
pub fn find_settings(pid: u32) -> Option<HWND> {
    find_window(pid, w!("LexTrail 设置"))
}

fn find_window(pid: u32, title: windows::core::PCWSTR) -> Option<HWND> {
    let mut after: Option<HWND> = None;
    loop {
        // SAFETY: plain Win32 calls with valid string literals.
        let hwnd = unsafe { FindWindowExW(None, after, w!("Tauri Window"), title) }.ok()?;
        let mut owner = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) };
        if owner == pid {
            return Some(hwnd);
        }
        after = Some(hwnd);
    }
}

/// Asks a window to close, as its close button would.
pub fn close(hwnd: HWND) -> Result<()> {
    // SAFETY: plain Win32 call.
    unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }?;
    Ok(())
}

/// Waits until the window is destroyed.
pub fn wait_gone(hwnd: HWND, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    // SAFETY: plain Win32 call; a destroyed handle reports false.
    while unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        if start.elapsed() > timeout {
            bail!("window did not close within {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

pub fn is_visible(hwnd: HWND) -> bool {
    // SAFETY: plain Win32 call; a stale handle just reports false.
    unsafe { IsWindowVisible(hwnd) }.as_bool()
}

/// Waits until the popup of `pid` has keyboard focus, so a following key press goes to
/// it. The app focuses the popup just after showing it.
pub fn wait_popup_focused(pid: u32, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    loop {
        // SAFETY: plain Win32 call.
        let foreground = unsafe { GetForegroundWindow() };
        if find_popup(pid) == Some(foreground) {
            return Ok(());
        }
        if start.elapsed() > timeout {
            bail!("popup did not get focus within {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Waits until the popup of `pid` is visible (`want = true`) or hidden / gone, and
/// returns the time since `since`. Spins instead of sleeping: `Sleep(1)` has ~15 ms
/// granularity, which would swamp the numbers being measured.
pub fn wait_popup(pid: u32, want: bool, since: Instant, timeout: Duration) -> Result<Duration> {
    loop {
        let visible = find_popup(pid).is_some_and(is_visible);
        if visible == want {
            return Ok(since.elapsed());
        }
        if since.elapsed() > timeout {
            bail!(
                "popup did not become {} within {timeout:?}",
                if want { "visible" } else { "hidden" }
            );
        }
        std::thread::yield_now();
    }
}
