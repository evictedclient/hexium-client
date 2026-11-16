//! Desktop notifications for chat messages that arrive over the hub.
//!
//! The decision is made on the hub's own thread, not in the UI loop: a minimised
//! window may not redraw, and the toast should not wait for it. The UI keeps a
//! small shared [`Gate`] current (your setting, whether the window has focus,
//! which conversation is on screen, everyone's names) and the hub callback reads
//! it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// At most one toast per conversation in this window, so a burst of messages
/// is one notification rather than ten.
const QUIET: Duration = Duration::from_secs(4);

#[derive(Default)]
pub struct Gate {
    pub enabled: bool,
    pub me: u64,
    pub focused: bool,
    /// The conversation open on the Chat screen, when that screen is showing.
    pub watching: Option<u64>,
    /// Trades, friend requests and messages notifications switched on.
    pub activity_enabled: bool,
    /// The screen on show (its menu label), when the window has focus.
    pub screen: &'static str,
    /// Conversations with unread messages, for the tray tooltip.
    pub unread: std::collections::HashSet<u64>,
    /// User id -> name, from the conversation list.
    pub names: HashMap<u64, String>,
    /// Conversation id -> name shown for it, for group conversations.
    pub groups: HashMap<u64, String>,
    last: HashMap<u64, Instant>,
}

pub type SharedGate = Arc<Mutex<Gate>>;

/// Learns names (and group titles) from the chat list.
pub fn learn(gate: &mut Gate, bundle: &crate::api::models::ChatBundle, me: u64) {
    for conversation in &bundle.conversations {
        for participant in &conversation.participants {
            if !participant.name.is_empty() {
                gate.names.insert(participant.id, participant.name.clone());
            }
        }
        if conversation.participants.len() > 2 {
            gate.groups.insert(conversation.id, conversation.label(me));
        }
    }
    for friend in &bundle.friends {
        gate.names.entry(friend.id).or_insert_with(|| friend.name.clone());
    }
    gate.unread = bundle
        .conversations
        .iter()
        .filter(|conversation| conversation.unread)
        .map(|conversation| conversation.id)
        .collect();
}

/// A message arrived over the hub: unless you are looking at that conversation,
/// it is unread now. Returns how many conversations are unread.
pub fn mark_unread(gate: &mut Gate, conversation: u64, sender: u64) -> u64 {
    let looking = gate.focused && gate.watching == Some(conversation);
    if sender != 0 && sender != gate.me && !looking {
        gate.unread.insert(conversation);
    }
    gate.unread.len() as u64
}

/// What a toast would say, or `None` when no toast should show.
pub fn decide(
    gate: &mut Gate,
    conversation: u64,
    sender: u64,
    content: &str,
    now: Instant,
) -> Option<(String, String)> {
    if !gate.enabled || sender == 0 || sender == gate.me || content.trim().is_empty() {
        return None;
    }
    // You are looking at this conversation: it shows up on screen anyway.
    if gate.focused && gate.watching == Some(conversation) {
        return None;
    }
    if gate
        .last
        .get(&conversation)
        .is_some_and(|at| now.duration_since(*at) < QUIET)
    {
        return None;
    }
    gate.last.insert(conversation, now);

    let name = gate
        .names
        .get(&sender)
        .cloned()
        .unwrap_or_else(|| "New chat message".to_string());
    let title = match gate.groups.get(&conversation) {
        Some(group) => format!("{name} in {group}"),
        None => name,
    };
    let mut body: String = content.trim().chars().take(200).collect();
    if content.trim().chars().count() > 200 {
        body.push('…');
    }
    Some((title, body))
}

/// Shows a Windows toast without blocking the caller, and runs `on_click` if the
/// toast itself is clicked. A plain thread waits for that (until the toast is
/// clicked or goes away), so closing the app never waits on it.
pub fn show<F>(title: String, body: String, on_click: F)
where
    F: FnOnce() + Send + 'static,
{
    std::thread::spawn(move || {
        // Best effort: a toast that fails to show changes nothing else.
        let mut toast = notify_rust::Notification::new();
        // Our own id only once the shortcut that defines it exists.
        #[cfg(windows)]
        if crate::shortcut::registered() {
            toast.app_id(crate::shortcut::APP_ID);
        }
        let handle = toast
            .appname("hexium client")
            .summary(&title)
            .body(&body)
            .timeout(notify_rust::Timeout::Milliseconds(6000))
            .show();
        let Ok(handle) = handle else { return };
        let _ = handle.wait_for_response(|response: &notify_rust::NotificationResponse| {
            if matches!(response, notify_rust::NotificationResponse::Default) {
                on_click();
            }
        });
    });
}

/// Restores and raises the window. Done straight from the toast thread with
/// the native handle: a minimised window may not be drawing, so a request
/// through the UI loop alone could sit until it is.
#[cfg(windows)]
pub fn bring_to_front(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW};
    if hwnd == 0 {
        return;
    }
    let window = HWND(hwnd as *mut core::ffi::c_void);
    unsafe {
        let command = if IsIconic(window).as_bool() { SW_RESTORE } else { SW_SHOW };
        let _ = ShowWindow(window, command);
        let _ = SetForegroundWindow(window);
    }
}

#[cfg(not(windows))]
pub fn bring_to_front(_hwnd: isize) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> Gate {
        let mut gate = Gate {
            enabled: true,
            me: 1,
            focused: true,
            watching: Some(10),
            ..Default::default()
        };
        gate.names.insert(2, "Friend".to_string());
        gate
    }

    #[test]
    fn quiet_for_the_open_conversation_while_focused() {
        let mut gate = gate();
        let now = Instant::now();
        assert!(decide(&mut gate, 10, 2, "hi", now).is_none());
        gate.focused = false;
        assert_eq!(
            decide(&mut gate, 10, 2, "hi", now),
            Some(("Friend".to_string(), "hi".to_string()))
        );
    }

    #[test]
    fn other_screen_own_messages_switch_and_burst() {
        let mut gate = gate();
        let now = Instant::now();
        gate.watching = None;
        assert!(decide(&mut gate, 10, 1, "mine", now).is_none());
        assert!(decide(&mut gate, 10, 2, "one", now).is_some());
        assert!(decide(&mut gate, 10, 2, "two", now + Duration::from_secs(1)).is_none());
        assert!(decide(&mut gate, 10, 2, "three", now + Duration::from_secs(5)).is_some());
        gate.enabled = false;
        assert!(decide(&mut gate, 11, 2, "off", now + Duration::from_secs(9)).is_none());
    }
}
