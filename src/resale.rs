//! The resale bot: each round it reads your collectibles and, for every copy,
//! looks at the cheapest listing someone else has and lists (or reprices) yours
//! just under it. It only ever writes a price: it never buys anything, never
//! takes a copy off sale, and never lists under the floor in Settings.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::api::{endpoints, ApiClient};
use crate::bot::{LogKind, LogLine};
use crate::config::ResaleConfig;

const LOG_LINES: usize = 200;
/// How many prices the screen keeps.
const ROWS: usize = 60;
/// A pause after each item's reads, so a long inventory is not a burst.
const BETWEEN: Duration = Duration::from_millis(400);

/// One price the bot set.
#[derive(Debug, Clone)]
pub struct Listing {
    pub asset: u64,
    pub name: String,
    pub price: u64,
    /// What your copy asked before; `None` when it was not on sale.
    pub previous: Option<u64>,
    /// The cheapest other listing it went under.
    pub lowest_other: u64,
    pub at: Instant,
}

#[derive(Default)]
pub struct State {
    pub running: bool,
    pub phase: String,
    pub next_round: Option<Instant>,
    pub log: VecDeque<LogLine>,
    pub listed: usize,
    pub repriced: usize,
    /// Copies already at or under the target, left alone.
    pub unchanged: usize,
    /// Copies with no market, no resale, or a market at the floor.
    pub skipped: usize,
    pub failed: usize,
    pub rounds: u64,
    /// The last prices it set, newest first.
    pub recent: VecDeque<Listing>,
}

impl State {
    fn log(&mut self, kind: LogKind, text: impl Into<String>) {
        self.log.push_front(LogLine { at: Instant::now(), kind, text: text.into() });
        self.log.truncate(LOG_LINES);
    }
}

/// Shared between the screen, which edits the settings, and the task.
#[derive(Default)]
pub struct Shared {
    pub settings: Mutex<ResaleConfig>,
    /// Item ids on the trade bot's keep list, skipped when `skip_kept` is on.
    pub keep: Mutex<HashSet<u64>>,
    pub state: Mutex<State>,
    stop: AtomicBool,
}

impl Shared {
    pub fn new(settings: ResaleConfig) -> Arc<Self> {
        Arc::new(Self { settings: Mutex::new(settings), ..Default::default() })
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.state.lock().unwrap().phase = "Stopping…".to_string();
    }

    pub fn run_now(&self) {
        self.state.lock().unwrap().next_round = Some(Instant::now());
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn phase(&self, text: &str) {
        self.state.lock().unwrap().phase = text.to_string();
    }

    fn log(&self, kind: LogKind, text: impl Into<String>) {
        self.state.lock().unwrap().log(kind, text);
    }
}

/// Starts the bot's task for the signed-in user. Does nothing if it already runs.
pub fn start(shared: Arc<Shared>, api: Arc<ApiClient>, me: u64, rt: &tokio::runtime::Handle, egui: egui::Context) {
    {
        let mut state = shared.state.lock().unwrap();
        if state.running {
            return;
        }
        state.running = true;
        state.phase = "Starting".to_string();
        state.next_round = Some(Instant::now());
        state.log(LogKind::Info, "Resale bot started.");
    }
    shared.stop.store(false, Ordering::Relaxed);
    rt.spawn(async move {
        run(&shared, &api, me, &egui).await;
        let mut state = shared.state.lock().unwrap();
        state.running = false;
        state.phase = "Stopped".to_string();
        state.next_round = None;
        state.log(LogKind::Info, "Resale bot stopped.");
        egui.request_repaint();
    });
}

async fn run(shared: &Shared, api: &ApiClient, me: u64, egui: &egui::Context) {
    while !shared.stopped() {
        let settings = shared.settings.lock().unwrap().clone();
        let due = shared.state.lock().unwrap().next_round.is_some_and(|at| Instant::now() >= at);
        if due {
            round(shared, api, me, &settings, egui).await;
            let mut state = shared.state.lock().unwrap();
            state.rounds += 1;
            state.next_round = Some(Instant::now() + Duration::from_secs(settings.cycle_minutes.max(1) * 60));
            state.phase = "Waiting for the next round".to_string();
            egui.request_repaint();
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        // The countdown on screen moves once a second.
        egui.request_repaint();
    }
}

async fn round(shared: &Shared, api: &ApiClient, me: u64, settings: &ResaleConfig, egui: &egui::Context) {
    shared.phase("Reading your inventory");
    let keep = shared.keep.lock().map(|keep| keep.clone()).unwrap_or_default();
    let items = match endpoints::collectibles_all(api, me).await {
        Ok(items) => items,
        Err(error) => {
            shared.log(LogKind::Problem, format!("Could not read your inventory: {error}"));
            return;
        }
    };
    let total = items.len();
    shared.log(LogKind::Info, format!("Scanning {total} collectible(s)."));
    for (index, item) in items.iter().enumerate() {
        if shared.stopped() {
            return;
        }
        if settings.skip_kept && keep.contains(&item.id) {
            shared.state.lock().unwrap().skipped += 1;
            continue;
        }
        shared.phase(&format!("Checking {} ({}/{total})", item.name, index + 1));
        egui.request_repaint();
        let info = match endpoints::resale_info(api, me, item.id, item.user_asset_id).await {
            Ok(info) => info,
            Err(error) => {
                shared.state.lock().unwrap().failed += 1;
                shared.log(LogKind::Problem, format!("{}: {error}", item.name));
                tokio::time::sleep(BETWEEN).await;
                continue;
            }
        };
        // Not something the site lets you sell, or nobody else selling one:
        // there is no price to go under, so it is left as it is.
        let Some(lowest) = info.lowest_other.filter(|_| info.resellable) else {
            shared.state.lock().unwrap().skipped += 1;
            continue;
        };
        let Some(target) = target_price(lowest, settings.undercut, settings.min_price) else {
            shared.state.lock().unwrap().skipped += 1;
            continue;
        };
        if !should_list(info.my_price, target) {
            shared.state.lock().unwrap().unchanged += 1;
            continue;
        }
        shared.phase(&format!("Listing {} at {} R$", item.name, target));
        egui.request_repaint();
        // One write at a time, with a pause: the site's write limit is shared
        // with whatever else the client is doing.
        tokio::time::sleep(Duration::from_secs(settings.gap_secs.max(1))).await;
        if shared.stopped() {
            return;
        }
        match endpoints::set_resale_price(api, item.id, item.user_asset_id, target).await {
            Ok(()) => {
                let previous = info.my_price;
                let mut state = shared.state.lock().unwrap();
                if previous.is_some() {
                    state.repriced += 1;
                } else {
                    state.listed += 1;
                }
                state.recent.push_front(Listing {
                    asset: item.id,
                    name: item.name.clone(),
                    price: target,
                    previous,
                    lowest_other: lowest,
                    at: Instant::now(),
                });
                state.recent.truncate(ROWS);
                state.log(
                    if previous.is_some() { LogKind::Good } else { LogKind::Sent },
                    format!(
                        "{} \"{}\" at {} R$ (cheapest other {})",
                        if previous.is_some() { "Repriced" } else { "Listed" },
                        item.name,
                        target,
                        lowest
                    ),
                );
                drop(state);
            }
            Err(error) => {
                shared.state.lock().unwrap().failed += 1;
                shared.log(LogKind::Problem, format!("Could not list \"{}\": {error}", item.name));
            }
        }
        tokio::time::sleep(BETWEEN).await;
    }
    shared.phase("Round finished");
}

/// The price to ask: `undercut` below the cheapest other listing, never under
/// `min_price`, and never 0 (0 takes a copy off sale instead of listing it).
pub fn target_price(lowest_other: u64, undercut: u64, min_price: u64) -> Option<u64> {
    let floor = min_price.max(1);
    let target = lowest_other.saturating_sub(undercut.max(1));
    (target >= floor).then_some(target)
}

/// Whether a price has to be written. A copy already at or under the target is
/// left alone: repricing it would only walk the price down against yourself.
pub fn should_list(current: Option<u64>, target: u64) -> bool {
    current.map_or(true, |mine| mine > target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undercuts_the_cheapest_other_listing() {
        assert_eq!(target_price(100, 1, 1), Some(99));
        assert_eq!(target_price(1_000, 5, 1), Some(995));
        // Rounds never go below the floor, and never reach 0.
        assert_eq!(target_price(1, 1, 1), None);
        assert_eq!(target_price(3, 1, 5), None);
        assert_eq!(target_price(3, 1, 0), Some(2));
    }

    #[test]
    fn leaves_a_copy_that_is_already_cheapest_alone() {
        assert!(should_list(None, 99));
        assert!(should_list(Some(120), 99));
        assert!(!should_list(Some(99), 99));
        assert!(!should_list(Some(80), 99));
    }
}
