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
    /// User id -> name, from the conversation list.
    pub names: HashMap<u64, String>,
    /// Conversation id -> name shown for it, for group conversations.
    pub groups: HashMap<u64, String>,
    last: HashMap<u64, Instant>,
}

pub type SharedGate = Arc<Mutex<Gate>>;

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

/// Shows a Windows toast without blocking the caller.
pub fn show(rt: &tokio::runtime::Handle, title: String, body: String) {
    rt.spawn_blocking(move || {
        // Best effort: a toast that fails to show changes nothing else.
        let _ = notify_rust::Notification::new()
            .appname("hexium client")
            .summary(&title)
            .body(&body)
            .timeout(notify_rust::Timeout::Milliseconds(6000))
            .show();
    });
}

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
