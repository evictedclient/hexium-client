//! The notification-area (tray) icon: Open and Quit in its menu, a click opens
//! the window, and the tooltip carries the unread chat count.
//!
//! It runs its own hidden window and message loop on a thread of its own, so it
//! keeps working while the main window is hidden and not drawing. The handle is
//! just numbers, so any thread can update the tooltip.

use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Open,
    Quit,
}

type Handler = Box<dyn Fn(Action) + Send + Sync>;
static HANDLER: OnceLock<Handler> = OnceLock::new();
/// The tray's own window, so the icon can be removed from anywhere.
static WINDOW: AtomicIsize = AtomicIsize::new(0);

/// Takes the icon away; safe to call more than once or with no tray.
pub fn remove_icon() {
    let window = WINDOW.swap(0, Ordering::Relaxed);
    if window != 0 {
        platform::remove(window);
    }
}

/// Hides the main window (it stays running, reachable from the tray).
pub fn hide_window(hwnd: isize) {
    platform::hide(hwnd);
}

/// The tooltip text for an unread count.
pub fn tooltip(unread: u64) -> String {
    match unread {
        0 => "hexium client".to_string(),
        1 => "hexium client · 1 unread chat".to_string(),
        count => format!("hexium client · {count} unread chats"),
    }
}

pub struct Tray {
    window: isize,
    shown_unread: AtomicU64,
}

impl Tray {
    /// Adds the icon. `on_action` runs on the tray's own thread.
    pub fn spawn<F>(icon: Option<std::path::PathBuf>, on_action: F) -> Option<Tray>
    where
        F: Fn(Action) + Send + Sync + 'static,
    {
        HANDLER.set(Box::new(on_action)).ok()?;
        let window = platform::spawn(icon)?;
        WINDOW.store(window, Ordering::Relaxed);
        Some(Tray {
            window,
            shown_unread: AtomicU64::new(0),
        })
    }

    /// Updates the tooltip when the count changed.
    pub fn set_unread(&self, unread: u64) {
        if self.shown_unread.swap(unread, Ordering::Relaxed) != unread {
            platform::set_tip(self.window, &tooltip(unread));
        }
    }

}

fn dispatch(action: Action) {
    if let Some(handler) = HANDLER.get() {
        handler(action);
    }
}

#[cfg(windows)]
mod platform {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc;

    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW, GetCursorPos,
        GetMessageW, GetSystemMetrics, LoadImageW, PostMessageW, RegisterClassW, RegisterWindowMessageW,
        SetForegroundWindow, TrackPopupMenu, TranslateMessage, HICON, IMAGE_ICON, LR_LOADFROMFILE,
        MF_SEPARATOR, MF_STRING, MSG, SM_CXSMICON, SM_CYSMICON, TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
        WINDOW_STYLE, WM_APP, WM_CONTEXTMENU, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL,
        WM_RBUTTONUP, WNDCLASSW, WS_EX_TOOLWINDOW,
    };

    use super::{dispatch, Action};

    const CALLBACK: u32 = WM_APP + 1;
    const ICON_ID: u32 = 1;
    const MENU_OPEN: usize = 1;
    const MENU_QUIT: usize = 2;

    /// Explorer's "the taskbar was recreated" broadcast; the icon must be re-added.
    static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
    static ICON: std::sync::Mutex<isize> = std::sync::Mutex::new(0);

    fn data(window: isize) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: HWND(window as *mut core::ffi::c_void),
            uID: ICON_ID,
            ..Default::default()
        }
    }

    fn put_tip(data: &mut NOTIFYICONDATAW, text: &str) {
        let wide: Vec<u16> = text.encode_utf16().take(data.szTip.len() - 1).collect();
        data.szTip[..wide.len()].copy_from_slice(&wide);
        data.szTip[wide.len()] = 0;
    }

    fn add(window: isize) {
        let mut entry = data(window);
        entry.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        entry.uCallbackMessage = CALLBACK;
        entry.hIcon = HICON(*ICON.lock().unwrap() as *mut core::ffi::c_void);
        put_tip(&mut entry, &super::tooltip(0));
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &entry);
        }
    }

    pub fn set_tip(window: isize, text: &str) {
        let mut entry = data(window);
        entry.uFlags = NIF_TIP;
        put_tip(&mut entry, text);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &entry);
        }
    }

    pub fn remove(window: isize) {
        let entry = data(window);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &entry);
        }
    }

    pub fn hide(hwnd: isize) {
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
        if hwnd != 0 {
            unsafe {
                let _ = ShowWindow(HWND(hwnd as *mut core::ffi::c_void), SW_HIDE);
            }
        }
    }

    fn show_menu(window: HWND) {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else { return };
            let _ = AppendMenuW(menu, MF_STRING, MENU_OPEN, w!("Open"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, MENU_QUIT, w!("Quit"));
            let mut point = POINT::default();
            let _ = GetCursorPos(&mut point);
            // Without this the menu does not close when you click elsewhere.
            let _ = SetForegroundWindow(window);
            let chosen = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
                point.x,
                point.y,
                None,
                window,
                None,
            );
            let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(menu);
            match chosen.0 as usize {
                MENU_OPEN => dispatch(Action::Open),
                MENU_QUIT => dispatch(Action::Quit),
                _ => {}
            }
        }
    }

    unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if message == CALLBACK {
            match (lparam.0 as u32) & 0xffff {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => dispatch(Action::Open),
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(window),
                _ => {}
            }
            return LRESULT(0);
        }
        let created = TASKBAR_CREATED.load(Ordering::Relaxed);
        if created != 0 && message == created {
            add(window.0 as isize);
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    /// Starts the tray thread; returns its window once the icon is added.
    pub fn spawn(icon: Option<std::path::PathBuf>) -> Option<isize> {
        let (ready_tx, ready_rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("tray".to_string())
            .spawn(move || unsafe {
                let instance = GetModuleHandleW(PCWSTR::null()).ok();
                if let Some(path) = &icon {
                    let loaded = LoadImageW(
                        None,
                        &HSTRING::from(path.as_os_str()),
                        IMAGE_ICON,
                        GetSystemMetrics(SM_CXSMICON),
                        GetSystemMetrics(SM_CYSMICON),
                        LR_LOADFROMFILE,
                    );
                    if let Ok(handle) = loaded {
                        *ICON.lock().unwrap() = handle.0 as isize;
                    }
                }
                let class = w!("HexiumClientTray");
                let description = WNDCLASSW {
                    lpfnWndProc: Some(procedure),
                    hInstance: instance.map(Into::into).unwrap_or_default(),
                    lpszClassName: class,
                    ..Default::default()
                };
                RegisterClassW(&description);
                TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);
                // A hidden top-level window, not a message-only one: message-only
                // windows never get the TaskbarCreated broadcast, so the icon would
                // vanish for good whenever Explorer restarts.
                let window = CreateWindowExW(
                    WS_EX_TOOLWINDOW,
                    class,
                    w!("hexium client tray"),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    instance.map(Into::into),
                    None,
                );
                let Ok(window) = window else {
                    let _ = ready_tx.send(None);
                    return;
                };
                add(window.0 as isize);
                let _ = ready_tx.send(Some(window.0 as isize));
                let mut message = MSG::default();
                while GetMessageW(&mut message, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            })
            .ok()?;
        ready_rx.recv().ok().flatten()
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn spawn(_icon: Option<std::path::PathBuf>) -> Option<isize> {
        None
    }
    pub fn set_tip(_window: isize, _text: &str) {}
    pub fn remove(_window: isize) {}
    pub fn hide(_hwnd: isize) {}
}

#[cfg(test)]
mod tests {
    use super::tooltip;

    #[test]
    fn tooltip_counts() {
        assert_eq!(tooltip(0), "hexium client");
        assert_eq!(tooltip(1), "hexium client · 1 unread chat");
        assert_eq!(tooltip(3), "hexium client · 3 unread chats");
    }
}

#[cfg(all(test, windows))]
mod live {
    /// Adds a real tray icon for a moment: `cargo test -- --ignored tray_spawns`.
    #[test]
    #[ignore]
    fn tray_spawns() {
        let tray = super::Tray::spawn(None, |_| {});
        assert!(tray.is_some(), "tray did not start");
        super::remove_icon();
    }
}
