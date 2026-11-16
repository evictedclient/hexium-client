//! "Open at sign-in": a value under the per-user `Run` key that starts the
//! client minimised (`--minimized`) when you log into Windows. No admin rights,
//! and nothing outside your own user hive.

/// The command the `Run` value should hold for this exe.
pub fn wanted_command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\" --minimized", exe.display()))
}

/// What Windows thinks of the entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Off,
    On,
    /// The value exists but was switched off in Task Manager's Startup apps.
    DisabledInTaskManager,
}

pub fn state() -> State {
    match platform::read() {
        None => State::Off,
        Some(_) if platform::disabled_in_task_manager() => State::DisabledInTaskManager,
        Some(_) => State::On,
    }
}

pub fn set(enabled: bool) -> Result<(), String> {
    if enabled {
        let command = wanted_command().ok_or("could not find the client's own exe")?;
        platform::write(&command)
    } else {
        platform::remove()
    }
}

/// If the entry is on but points at another exe (the client was moved or
/// rebuilt elsewhere), point it at this one.
pub fn refresh() {
    let (Some(current), Some(wanted)) = (platform::read(), wanted_command()) else { return };
    if current != wanted {
        let _ = platform::write(&wanted);
    }
}

#[cfg(windows)]
mod platform {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_BINARY,
        RRF_RT_REG_SZ,
    };

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    const NAME: &str = "hexium client";

    pub fn read() -> Option<String> {
        let mut buffer = vec![0u16; 2048];
        let mut bytes = (buffer.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(RUN),
                &HSTRING::from(NAME),
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let length = (bytes as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buffer[..length.min(buffer.len())]))
    }

    pub fn write(command: &str) -> Result<(), String> {
        let wide: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(RUN),
                &HSTRING::from(NAME),
                REG_SZ.0,
                Some(wide.as_ptr().cast()),
                (wide.len() * 2) as u32,
            )
        };
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("Windows refused the sign-in entry (error {})", status.0))
        }
    }

    pub fn remove() -> Result<(), String> {
        let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(RUN), &HSTRING::from(NAME)) };
        if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(format!("Windows refused to remove the sign-in entry (error {})", status.0))
        }
    }

    /// Task Manager keeps its own switch: an odd first byte means disabled.
    pub fn disabled_in_task_manager() -> bool {
        let mut buffer = [0u8; 12];
        let mut bytes = buffer.len() as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(APPROVED),
                &HSTRING::from(NAME),
                RRF_RT_REG_BINARY,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
        };
        status == ERROR_SUCCESS && bytes > 0 && buffer[0] & 1 == 1
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn read() -> Option<String> {
        None
    }
    pub fn write(_command: &str) -> Result<(), String> {
        Err("only available on Windows".to_string())
    }
    pub fn remove() -> Result<(), String> {
        Ok(())
    }
    pub fn disabled_in_task_manager() -> bool {
        false
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// Writes to the real `Run` key, so it only runs when asked:
    /// `cargo test -- --ignored sign_in_entry`. It leaves things as it found them.
    #[test]
    #[ignore]
    fn sign_in_entry_round_trip() {
        let before = platform::read();
        set(true).unwrap();
        assert_ne!(state(), State::Off);
        assert_eq!(platform::read(), wanted_command());
        set(false).unwrap();
        assert_eq!(state(), State::Off);
        if let Some(command) = before {
            platform::write(&command).unwrap();
        }
    }
}
