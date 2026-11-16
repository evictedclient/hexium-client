//! One client at a time. The first instance holds a named mutex and records its
//! window in a small file; a second launch (by hand, from the Start menu, or
//! from a notification clicked in Action Center) finds that window, brings it to
//! the front and exits.

use std::path::PathBuf;
use std::time::{Duration, Instant};

const MUTEX_NAME: &str = "Local\\evictedclient.HexiumClient";

fn record_path() -> PathBuf {
    crate::config::config_dir().join("instance")
}

/// `"<pid> <hwnd>"`.
fn parse_record(text: &str) -> Option<(u32, isize)> {
    let mut parts = text.split_whitespace();
    let pid = parts.next()?.parse().ok()?;
    let hwnd = parts.next()?.parse().ok()?;
    (pid != 0 && hwnd != 0).then_some((pid, hwnd))
}

/// Called by the running client once its window exists.
pub fn publish(hwnd: isize) {
    let path = record_path();
    if let Some(folder) = path.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let _ = std::fs::write(path, format!("{} {hwnd}", std::process::id()));
}

/// True when this process is the only client. Must be kept for the whole run,
/// which it is: the mutex handle is deliberately never closed.
pub fn claim() -> bool {
    platform::claim()
}

/// For a second launch: raise the running client's window. Waits briefly in
/// case the first one is still opening its window.
pub fn wake_existing() {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let record = std::fs::read_to_string(record_path()).ok();
        if let Some((pid, hwnd)) = record.as_deref().and_then(parse_record) {
            if platform::raise(pid, hwnd) {
                return;
            }
        }
        if Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(windows)]
mod platform {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow, ShowWindow,
        SW_RESTORE, SW_SHOW,
    };

    use super::MUTEX_NAME;

    pub fn claim() -> bool {
        unsafe {
            match CreateMutexW(None, false, &HSTRING::from(MUTEX_NAME)) {
                // The handle stays open until the process exits.
                Ok(_handle) => GetLastError() != ERROR_ALREADY_EXISTS,
                // Could not even ask: better two windows than none.
                Err(_) => true,
            }
        }
    }

    /// Raises `hwnd` if it is still a window of process `pid`.
    pub fn raise(pid: u32, hwnd: isize) -> bool {
        let window = HWND(hwnd as *mut core::ffi::c_void);
        unsafe {
            if !IsWindow(Some(window)).as_bool() {
                return false;
            }
            let mut owner = 0u32;
            GetWindowThreadProcessId(window, Some(&mut owner));
            // A recycled handle that now belongs to someone else.
            if owner != pid {
                return false;
            }
            // This freshly launched process may take the foreground; pass that on.
            let _ = AllowSetForegroundWindow(pid);
            let command = if IsIconic(window).as_bool() { SW_RESTORE } else { SW_SHOW };
            let _ = ShowWindow(window, command);
            let _ = SetForegroundWindow(window);
        }
        true
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn claim() -> bool {
        true
    }

    pub fn raise(_pid: u32, _hwnd: isize) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::parse_record;

    #[test]
    fn record_round_trip() {
        assert_eq!(parse_record("1234 5678"), Some((1234, 5678)));
        assert_eq!(parse_record("1234"), None);
        assert_eq!(parse_record("0 5678"), None);
        assert_eq!(parse_record("garbage"), None);
    }
}
