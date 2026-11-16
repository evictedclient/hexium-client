//! The limiteds watchlist: every two minutes, read the cheapest reseller of each
//! watched item and alert once when it reaches the target. It alerts again only
//! if the price drops further, and re-arms once the price goes back above the
//! target. It never buys anything.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::api::{endpoints, ApiClient};
use crate::config::WatchItem;
use crate::watch::WatchHandle;

const EVERY: Duration = Duration::from_secs(120);
/// A short pause between items, so a long list is not a burst of requests.
const BETWEEN: Duration = Duration::from_millis(400);

pub type SharedList = Arc<Mutex<Vec<WatchItem>>>;

/// Remembers what was alerted, so each drop is reported once.
#[derive(Default)]
pub struct Alerts {
    /// Asset id -> (target it was alerted for, price alerted at).
    sent: HashMap<u64, (u64, u64)>,
}

impl Alerts {
    /// Whether this price should alert now, recording it if so.
    pub fn should_alert(&mut self, item: &WatchItem, cheapest: Option<u64>) -> bool {
        let Some(price) = cheapest.filter(|price| *price > 0 && *price <= item.target) else {
            // Above the target (or nobody selling): ready to alert again.
            self.sent.remove(&item.asset_id);
            return false;
        };
        let fresh = match self.sent.get(&item.asset_id) {
            Some((target, last)) => *target != item.target || price < *last,
            None => true,
        };
        if fresh {
            self.sent.insert(item.asset_id, (item.target, price));
        }
        fresh
    }
}

/// `alert` gets an item that reached its target and the price; `seen` gets every
/// price read (for the list on screen).
pub fn spawn<A, S>(rt: &tokio::runtime::Handle, api: Arc<ApiClient>, list: SharedList, alert: A, seen: S) -> WatchHandle
where
    A: Fn(WatchItem, u64) + Send + Sync + 'static,
    S: Fn(u64, Option<u64>) + Send + Sync + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    rt.spawn(async move {
        let mut alerts = Alerts::default();
        while !flag.load(Ordering::Relaxed) {
            let items = list.lock().map(|list| list.clone()).unwrap_or_default();
            for item in items {
                if flag.load(Ordering::Relaxed) {
                    return;
                }
                if let Ok(cheapest) = endpoints::cheapest_resale(&api, item.asset_id).await {
                    seen(item.asset_id, cheapest);
                    if alerts.should_alert(&item, cheapest) {
                        if let Some(price) = cheapest {
                            alert(item.clone(), price);
                        }
                    }
                }
                tokio::time::sleep(BETWEEN).await;
            }
            let mut waited = Duration::ZERO;
            while waited < EVERY && !flag.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_secs(1)).await;
                waited += Duration::from_secs(1);
            }
        }
    });
    WatchHandle::from_flag(stop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(target: u64) -> WatchItem {
        WatchItem { asset_id: 7, name: "Thing".to_string(), target }
    }

    #[test]
    fn alerts_once_per_drop_and_rearms() {
        let mut alerts = Alerts::default();
        assert!(!alerts.should_alert(&item(500), Some(600)));
        assert!(alerts.should_alert(&item(500), Some(480)));
        // Same or higher (still under target): quiet.
        assert!(!alerts.should_alert(&item(500), Some(480)));
        assert!(!alerts.should_alert(&item(500), Some(490)));
        // Lower again: alert.
        assert!(alerts.should_alert(&item(500), Some(450)));
        // Back above, then down: alert again.
        assert!(!alerts.should_alert(&item(500), Some(700)));
        assert!(alerts.should_alert(&item(500), Some(495)));
        // A new target re-arms.
        assert!(alerts.should_alert(&item(600), Some(495)));
        // Nobody selling never alerts.
        assert!(!alerts.should_alert(&item(600), None));
    }
}
