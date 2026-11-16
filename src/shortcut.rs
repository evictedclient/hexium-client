//! Gives the client its own identity on Windows so notifications say
//! "hexium client" with its icon instead of "Windows PowerShell".
//!
//! Unpackaged desktop apps get that from a Start-menu shortcut carrying a
//! `System.AppUserModel.ID`; the running process then claims the same id. The
//! shortcut lives in the per-user Start menu (no admin rights) and is only
//! rewritten when the exe path changes or the file is missing.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// The AppUserModelID shared by the shortcut, the process and every toast.
pub const APP_ID: &str = "evictedclient.HexiumClient";
const SHORTCUT_NAME: &str = "hexium client.lnk";

static REGISTERED: AtomicBool = AtomicBool::new(false);

/// True once the shortcut is in place, so toasts can use [`APP_ID`]. Until then
/// they must not: Windows drops toasts from an id it has never seen.
pub fn registered() -> bool {
    REGISTERED.load(Ordering::Relaxed)
}

/// Writes the icon file next to the config. Returns its path.
pub fn write_icon() -> Option<PathBuf> {
    let path = crate::config::config_dir().join("hexium.ico");
    let bytes = crate::icon::ico();
    let current = std::fs::read(&path).ok();
    if current.as_deref() != Some(bytes.as_slice()) {
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::write(&path, &bytes).ok()?;
    }
    Some(path)
}

/// Claims the id for this process and makes sure the shortcut exists.
/// Failures are returned as text and leave toasts on the fallback id.
pub fn register(config: &mut crate::config::Config) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| format!("no exe path: {error}"))?;
    let icon = write_icon().ok_or("could not write the icon file")?;
    let link = shortcut_path().ok_or("no Start menu folder")?;

    platform::set_process_id()?;

    let wanted = exe.display().to_string();
    let fresh = link.exists() && config.shortcut_exe.as_deref() == Some(wanted.as_str());
    if !fresh {
        platform::write_shortcut(&link, &exe, &icon)?;
        config.shortcut_exe = Some(wanted);
        config.save();
    }
    REGISTERED.store(true, Ordering::Relaxed);
    Ok(())
}

fn shortcut_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        Path::new(&appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join(SHORTCUT_NAME),
    )
}

#[cfg(windows)]
mod platform {
    use std::path::Path;

    use windows::core::{Interface, HSTRING};
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::{IShellLinkW, SetCurrentProcessExplicitAppUserModelID, ShellLink};

    use super::APP_ID;

    pub fn set_process_id() -> Result<(), String> {
        unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(APP_ID)) }
            .map_err(|error| format!("could not set the app id: {error}"))
    }

    /// COM runs on its own short-lived thread so it cannot clash with whatever
    /// the window toolkit set up on the main one.
    pub fn write_shortcut(link: &Path, exe: &Path, icon: &Path) -> Result<(), String> {
        let (link, exe, icon) = (link.to_path_buf(), exe.to_path_buf(), icon.to_path_buf());
        std::thread::spawn(move || unsafe {
            let initialised = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
            let result = (|| -> windows::core::Result<()> {
                let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                shell_link.SetPath(&HSTRING::from(exe.as_os_str()))?;
                if let Some(folder) = exe.parent() {
                    shell_link.SetWorkingDirectory(&HSTRING::from(folder.as_os_str()))?;
                }
                shell_link.SetDescription(&HSTRING::from("Desktop client for hexium.wtf"))?;
                shell_link.SetIconLocation(&HSTRING::from(icon.as_os_str()), 0)?;

                let store: IPropertyStore = shell_link.cast()?;
                store.SetValue(&PKEY_AppUserModel_ID, &PROPVARIANT::from(APP_ID))?;
                store.Commit()?;

                let file: IPersistFile = shell_link.cast()?;
                file.Save(&HSTRING::from(link.as_os_str()), true)?;
                Ok(())
            })();
            if initialised {
                CoUninitialize();
            }
            result.map_err(|error| format!("could not write the shortcut: {error}"))
        })
        .join()
        .map_err(|_| "the shortcut thread panicked".to_string())?
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::Path;

    pub fn set_process_id() -> Result<(), String> {
        Err("only needed on Windows".to_string())
    }

    pub fn write_shortcut(_link: &Path, _exe: &Path, _icon: &Path) -> Result<(), String> {
        Err("only needed on Windows".to_string())
    }
}
