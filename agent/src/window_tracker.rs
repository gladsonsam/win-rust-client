//! # Window Focus Tracker
//!
//! Detects foreground-window switches and emits a [`WindowEvent`] on every
//! title or HWND change.
//!
//! ## Why `QueryFullProcessImageNameW` instead of `GetWindowModuleFileNameW`
//!
//! `GetWindowModuleFileNameW` returns an empty string for modern packaged /
//! UWP apps (Microsoft Edge, Microsoft Store apps, etc.) because those
//! processes load through a host process that Windows doesn't expose via that
//! API.  `QueryFullProcessImageNameW` works for all processes, including
//! packaged ones, as long as we open the process with
//! `PROCESS_QUERY_LIMITED_INFORMATION` (which is granted even cross-elevation).

use windows::{
    Win32::{
        Foundation::{CloseHandle, HWND},
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
        UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
        },
    },
    core::PWSTR,
};

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WindowEvent {
    /// Full window title (e.g. `"Rust docs – Google Chrome"`).
    pub title: String,
    /// Short executable name (e.g. `"msedge.exe"`, `"chrome.exe"`).
    pub app: String,
    /// Raw HWND value for server-side correlation.
    pub hwnd: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Tracker
// ─────────────────────────────────────────────────────────────────────────────

pub struct WindowTracker {
    last_hwnd:  usize,
    last_title: String,
}

impl Default for WindowTracker {
    fn default() -> Self {
        Self { last_hwnd: 0, last_title: String::new() }
    }
}

impl WindowTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `Some(WindowEvent)` when the foreground window or its title
    /// has changed since the last call; `None` otherwise.
    pub fn poll(&mut self) -> Option<WindowEvent> {
        let hwnd: HWND = unsafe { GetForegroundWindow() };
        let hwnd_raw = hwnd.0 as usize;

        // Null HWND → desktop has focus. Emit once on transition.
        if hwnd_raw == 0 {
            if self.last_hwnd != 0 {
                self.last_hwnd  = 0;
                self.last_title = String::new();
                return Some(WindowEvent { title: String::new(), app: String::new(), hwnd: 0 });
            }
            return None;
        }

        let title = read_window_title(hwnd);

        if hwnd_raw == self.last_hwnd && title == self.last_title {
            return None;
        }

        let app = read_process_name(hwnd);

        self.last_hwnd  = hwnd_raw;
        self.last_title = title.clone();

        Some(WindowEvent { title, app, hwnd: hwnd_raw })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Win32 helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Read the window title via `GetWindowTextW` (slice-based API in windows 0.58).
fn read_window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if len <= 0 { return String::new(); }
    String::from_utf16_lossy(&buf[..len as usize])
}

/// Read the executable basename using `QueryFullProcessImageNameW`.
///
/// This works for **all** process types including packaged/UWP apps like
/// Microsoft Edge, unlike `GetWindowModuleFileNameW` which returns empty
/// for those processes.
///
/// Steps:
/// 1. `GetWindowThreadProcessId` → PID
/// 2. `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` → handle
/// 3. `QueryFullProcessImageNameW` → full path (e.g. `C:\...\msedge.exe`)
/// 4. Split on `\` / `/` → just the filename
fn read_process_name(hwnd: HWND) -> String {
    // ── 1. PID ───────────────────────────────────────────────────────────
    let mut pid: u32 = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 { return String::new(); }

    // ── 2. Process handle ────────────────────────────────────────────────
    let handle = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(h)  => h,
        Err(_) => return String::new(),
    };

    // ── 3. Full image path ───────────────────────────────────────────────
    let mut buf  = [0u16; 1024];
    let mut size = buf.len() as u32;
    let result   = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0), // Win32 path format (not NT native)
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
    };
    // Always close the handle, regardless of success.
    let _ = unsafe { CloseHandle(handle) };

    if result.is_err() { return String::new(); }

    // ── 4. Strip path → basename ─────────────────────────────────────────
    let full_path = String::from_utf16_lossy(&buf[..size as usize]);
    full_path
        .rsplit(|c| c == '\\' || c == '/')
        .next()
        .unwrap_or("")
        .to_string()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "windows")]
    fn first_poll_returns_event() {
        let mut tracker = WindowTracker::new();
        assert!(tracker.poll().is_some());
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn second_consecutive_poll_returns_none() {
        let mut tracker = WindowTracker::new();
        let _ = tracker.poll();
        assert!(tracker.poll().is_none());
    }
}
