//! Watches for new inbound trades, friend requests and private messages, which
//! have no live push like chat does. Every minute it reads the three counters
//! the site's own badges use; only when one changes does it fetch that list and
//! report the items it has not seen before. The first round just records what
//! is already there.
//!
//! Runs on the tokio runtime, so it keeps going while the window is hidden.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::api::{endpoints, ApiClient};

const EVERY: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Trade,
    FriendRequest,
    Message,
}

/// One new thing: who it is from and a line about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub from: String,
    pub detail: String,
}

/// What a notification for `items` of one kind says. More than three at once
/// become one summary.
pub fn toast_text(kind: Kind, items: &[Item]) -> Option<(String, String)> {
    let first = items.first()?;
    if items.len() > 3 {
        let noun = match kind {
            Kind::Trade => "trades",
            Kind::FriendRequest => "friend requests",
            Kind::Message => "messages",
        };
        let names: Vec<&str> = items.iter().take(3).map(|item| item.from.as_str()).collect();
        return Some((
            format!("{} new {noun}", items.len()),
            format!("From {} and others", names.join(", ")),
        ));
    }
    let title = match kind {
        Kind::Trade => format!("Trade from {}", first.from),
        Kind::FriendRequest => format!("Friend request from {}", first.from),
        Kind::Message => format!("Message from {}", first.from),
    };
    let mut body = first.detail.clone();
    if items.len() > 1 {
        body.push_str(&format!(" (+{} more)", items.len() - 1));
    }
    Some((title, body))
}

/// What has been seen of one list.
#[derive(Default)]
struct Seen {
    count: Option<u64>,
    ids: HashSet<u64>,
    started: bool,
}

impl Seen {
    /// Records `ids`; returns the positions of ones not seen before (none on the
    /// first call, which only records).
    fn fresh(&mut self, ids: &[u64]) -> Vec<usize> {
        let first = !self.started;
        self.started = true;
        let mut fresh = Vec::new();
        for (index, id) in ids.iter().enumerate() {
            if self.ids.insert(*id) && !first {
                fresh.push(index);
            }
        }
        fresh
    }
}

pub struct WatchHandle {
    stop: Arc<AtomicBool>,
}

impl WatchHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts the watcher. `report` gets each batch of new items of one kind;
/// `changed` is called whenever any counter moved (for the badges).
pub fn spawn<R, C>(rt: &tokio::runtime::Handle, api: Arc<ApiClient>, report: R, changed: C) -> WatchHandle
where
    R: Fn(Kind, Vec<Item>) + Send + Sync + 'static,
    C: Fn() + Send + Sync + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    rt.spawn(async move {
        let mut trades = Seen::default();
        let mut requests = Seen::default();
        let mut messages = Seen::default();
        while !flag.load(Ordering::Relaxed) {
            let (trade_count, request_count, message_count) = tokio::join!(
                endpoints::inbound_trade_count(&api),
                endpoints::friend_request_count(&api),
                endpoints::unread_message_count(&api),
            );
            let mut moved = false;

            if let Ok(count) = trade_count {
                if trades.count != Some(count) {
                    if let Ok(page) = endpoints::trades(&api, "inbound", None).await {
                        let ids: Vec<u64> = page.items.iter().map(|trade| trade.id).collect();
                        let fresh: Vec<Item> = trades
                            .fresh(&ids)
                            .into_iter()
                            .map(|index| Item {
                                from: page.items[index].partner_name.clone(),
                                detail: "Open Trades to review it.".to_string(),
                            })
                            .collect();
                        moved |= trades.count.is_some();
                        trades.count = Some(count);
                        if !fresh.is_empty() {
                            report(Kind::Trade, fresh);
                        }
                    }
                }
            }

            if let Ok(count) = request_count {
                if requests.count != Some(count) {
                    if let Ok(bundle) = endpoints::friend_requests(&api).await {
                        let ids: Vec<u64> = bundle.items.iter().map(|request| request.user_id).collect();
                        let fresh: Vec<Item> = requests
                            .fresh(&ids)
                            .into_iter()
                            .map(|index| Item {
                                from: bundle.items[index].name.clone(),
                                detail: "Wants to be your friend.".to_string(),
                            })
                            .collect();
                        moved |= requests.count.is_some();
                        requests.count = Some(count);
                        if !fresh.is_empty() {
                            report(Kind::FriendRequest, fresh);
                        }
                    }
                }
            }

            if let Ok(count) = message_count {
                if messages.count != Some(count) {
                    if let Ok(page) = endpoints::messages(&api, "inbox", 0).await {
                        let ids: Vec<u64> = page.items.iter().map(|message| message.id).collect();
                        let fresh: Vec<Item> = messages
                            .fresh(&ids)
                            .into_iter()
                            .filter(|index| !page.items[*index].is_read)
                            .map(|index| {
                                let message = &page.items[index];
                                let from = if message.sender_name.is_empty() {
                                    "hexium.wtf".to_string()
                                } else {
                                    message.sender_name.clone()
                                };
                                let subject = if message.subject.trim().is_empty() {
                                    "(no subject)".to_string()
                                } else {
                                    message.subject.trim().to_string()
                                };
                                Item { from, detail: subject }
                            })
                            .collect();
                        moved |= messages.count.is_some();
                        messages.count = Some(count);
                        if !fresh.is_empty() {
                            report(Kind::Message, fresh);
                        }
                    }
                }
            }

            if moved {
                changed();
            }
            // Sleep in short steps so sign-out stops it promptly.
            let mut waited = Duration::ZERO;
            while waited < EVERY && !flag.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_secs(1)).await;
                waited += Duration::from_secs(1);
            }
        }
    });
    WatchHandle { stop }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_round_only_records() {
        let mut seen = Seen::default();
        assert!(seen.fresh(&[1, 2]).is_empty());
        assert_eq!(seen.fresh(&[3, 1, 2]), vec![0]);
        assert!(seen.fresh(&[3, 1]).is_empty());
    }

    #[test]
    fn toast_wording() {
        let item = |name: &str| Item { from: name.to_string(), detail: "Hi".to_string() };
        assert_eq!(
            toast_text(Kind::Trade, &[item("A")]),
            Some(("Trade from A".to_string(), "Hi".to_string()))
        );
        assert_eq!(
            toast_text(Kind::Message, &[item("A"), item("B")]),
            Some(("Message from A".to_string(), "Hi (+1 more)".to_string()))
        );
        let many = [item("A"), item("B"), item("C"), item("D")];
        assert_eq!(
            toast_text(Kind::FriendRequest, &many),
            Some(("4 new friend requests".to_string(), "From A, B, C and others".to_string()))
        );
        assert!(toast_text(Kind::Trade, &[]).is_none());
    }
}
