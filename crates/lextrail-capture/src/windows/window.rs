//! Foreground window snapshot (plain Win32, no cross-process calls that can hang).

use std::ffi::c_void;
use std::mem::size_of;

use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TOKEN_MANDATORY_LABEL,
    TOKEN_QUERY, TokenIntegrityLevel,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GUITHREADINFO, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowTextW,
    GetWindowThreadProcessId,
};
use windows::core::{Owned, PWSTR};

use crate::{CaptureTarget, Integrity, PlatformTarget, WindowInfo};

pub(crate) fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut c_void)
}

pub(super) fn snapshot() -> CaptureTarget {
    // SAFETY: plain Win32 call.
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_invalid() {
        return CaptureTarget {
            window: None,
            platform: PlatformTarget::default(),
        };
    }
    let mut process_id = 0;
    // SAFETY: valid out pointer.
    let thread_id = unsafe { GetWindowThreadProcessId(foreground, Some(&mut process_id)) };

    let mut gui = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `gui` is a properly sized GUITHREADINFO. On failure the handles stay 0.
    let _ = unsafe { GetGUIThreadInfo(thread_id, &mut gui) };

    CaptureTarget {
        window: Some(WindowInfo {
            process_id,
            process_name: process_name(process_id),
            class_name: class_name(foreground),
            title: title(foreground),
            integrity: integrity(process_id),
        }),
        platform: PlatformTarget {
            hwnd: foreground.0 as isize,
            hwnd_focus: gui.hwndFocus.0 as isize,
            hwnd_caret: gui.hwndCaret.0 as isize,
        },
    }
}

fn process_name(process_id: u32) -> Option<String> {
    // SAFETY: the handle is owned and closed on drop; the buffer outlives the call.
    unsafe {
        let process =
            Owned::new(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).ok()?);
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        QueryFullProcessImageNameW(
            *process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().map(str::to_owned)
    }
}

fn class_name(hwnd: HWND) -> Option<String> {
    let mut buf = [0u16; 256];
    // SAFETY: valid buffer.
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    (len > 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
}

fn title(hwnd: HWND) -> Option<String> {
    let mut buf = [0u16; 512];
    // SAFETY: valid buffer. For another process's window this reads the cached caption
    // without sending it a message, so a hung target cannot block us.
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    (len > 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
}

fn integrity(process_id: u32) -> Integrity {
    // SAFETY: the process handle is owned and closed on drop.
    let target = unsafe {
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id)
            .ok()
            .map(|h| Owned::new(h))
            .and_then(|process| integrity_level(*process))
    };
    // SAFETY: the pseudo handle needs no closing.
    let own = integrity_level(unsafe { GetCurrentProcess() });
    match (target, own) {
        (Some(target), Some(own)) if target > own => Integrity::Higher,
        (Some(_), Some(_)) => Integrity::SameOrLower,
        _ => Integrity::Unknown,
    }
}

/// The mandatory integrity level RID of `process` (e.g. 0x2000 medium, 0x3000 high).
fn integrity_level(process: HANDLE) -> Option<u32> {
    // SAFETY: handles are owned and closed on drop; the buffer is large enough for a
    // TOKEN_MANDATORY_LABEL with its SID and is suitably aligned (u64 storage).
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let token = Owned::new(token);
        let mut buf = [0u64; 16];
        let mut len = 0;
        GetTokenInformation(
            *token,
            TokenIntegrityLevel,
            Some(buf.as_mut_ptr().cast()),
            size_of_val(&buf) as u32,
            &mut len,
        )
        .ok()?;
        let label = &*buf.as_ptr().cast::<TOKEN_MANDATORY_LABEL>();
        let sid = label.Label.Sid;
        let count = *GetSidSubAuthorityCount(sid);
        if count == 0 {
            return None;
        }
        Some(*GetSidSubAuthority(sid, u32::from(count) - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_process_has_an_integrity_level_and_is_not_higher_than_itself() {
        // SAFETY: pseudo handle.
        assert!(integrity_level(unsafe { GetCurrentProcess() }).is_some());
        assert_eq!(integrity(std::process::id()), Integrity::SameOrLower);
    }

    #[test]
    fn own_process_name_is_found() {
        let name = process_name(std::process::id()).unwrap();
        assert!(name.ends_with(".exe"), "{name}");
    }

    #[test]
    fn unknown_process_gives_unknown_integrity() {
        // Process IDs are multiples of 4; this one never exists.
        assert_eq!(integrity(3), Integrity::Unknown);
        assert_eq!(process_name(3), None);
    }
}
