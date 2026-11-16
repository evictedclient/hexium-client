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
    /// A trade you sent was answered (accepted, declined, countered or expired).
    TradeResult,
}

/// What happened to a trade you sent, from its final `status`.
pub fn outcome(status: &str) -> &'static str {
    match status.to_ascii_lowercase().as_str() {
        "completed" | "accepted" => "accepted",
        "declined" | "rejected" => "declined",
        "countered" => "countered",
        "expired" => "expired",
        _ => "ended",
    }
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
    if kind == Kind::TradeResult {
        // One toast per answered trade reads best; several become a summary.
        if items.len() == 1 {
            return Some((format!("{} {} your trade", first.from, first.detail), trade_result_body(&first.detail)));
        }
        let names: Vec<&str> = items.iter().take(3).map(|item| item.from.as_str()).collect();
        return Some((format!("{} of your trades were answered", items.len()), format!("From {}", names.join(", "))));
    }
    if items.len() > 3 {
        let noun = match kind {
            Kind::Trade => "trades",
            Kind::FriendRequest => "friend requests",
            Kind::Message => "messages",
            Kind::TradeResult => "trade results",
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
        Kind::TradeResult => unreachable!("handled above"),
    };
    let mut body = first.detail.clone();
    if items.len() > 1 {
        body.push_str(&format!(" (+{} more)", items.len() - 1));
    }
    Some((title, body))
}

fn trade_result_body(outcome: &str) -> String {
    match outcome {
        "accepted" => "The items have moved.".to_string(),
        "declined" => "Nothing changed hands.".to_string(),
        "countered" => "Their counter offer is in your inbound trades.".to_string(),
        "expired" => "It ran out before they answered.".to_string(),
        _ => "It is no longer open.".to_string(),
    }
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
    pub fn from_flag(stop: Arc<AtomicBool>) -> Self {
        Self { stop }
    }

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
        // Open trades you sent, by id, with the partner's name; `None` until the
        // first round has recorded them.
        let mut outbound: Option<std::collections::HashMap<u64, String>> = None;
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

            // Trades you sent: one that leaves the open list was answered.
            if let Ok(page) = endpoints::trades(&api, "outbound", None).await {
                let open: std::collections::HashMap<u64, String> = page
                    .items
                    .iter()
                    .map(|trade| (trade.id, trade.partner_name.clone()))
                    .collect();
                if let Some(before) = &outbound {
                    let mut results = Vec::new();
                    for (id, partner) in before {
                        if open.contains_key(id) {
                            continue;
                        }
                        let status = endpoints::trade_detail(&api, *id)
                            .await
                            .map(|detail| detail.status)
                            .unwrap_or_default();
                        results.push(Item { from: partner.clone(), detail: outcome(&status).to_string() });
                    }
                    if !results.is_empty() {
                        moved = true;
                        report(Kind::TradeResult, results);
                    }
                }
                outbound = Some(open);
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

    #[test]
    fn trade_results() {
        assert_eq!(outcome("Completed"), "accepted");
        assert_eq!(outcome("Declined"), "declined");
        assert_eq!(outcome("Countered"), "countered");
        assert_eq!(outcome("Expired"), "expired");
        assert_eq!(outcome("RejectedDueToError"), "ended");
        let one = [Item { from: "A".to_string(), detail: "accepted".to_string() }];
        assert_eq!(
            toast_text(Kind::TradeResult, &one),
            Some(("A accepted your trade".to_string(), "The items have moved.".to_string()))
        );
        let two = [one[0].clone(), Item { from: "B".to_string(), detail: "declined".to_string() }];
        assert_eq!(toast_text(Kind::TradeResult, &two).map(|(title, _)| title), Some("2 of your trades were answered".to_string()));
    }
}
