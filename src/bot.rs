//! The trade bot: runs in the background while the app is open (also when it
//! is hidden in the tray). Each round it reads your inventory and inbound
//! trades, makes new trades from your items (upgrades, downgrades or any) and
//! counters inbound trades below your bar. Trades are sent one at a time with
//! a wait between them and an hourly limit; without auto-send they wait in a
//! review list until you approve them.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::api::{endpoints, models, ApiClient};
use crate::app::{check_players, order_players, Found, Msg, RangeRules, TradeKind, MASS_MIN_GAP, TRADE_SIDE_LIMIT};
use crate::config::{BotConfig, PlayerFilters};
use crate::values::Values;

const LOG_LINES: usize = 200;

/// One item in a bot trade.
#[derive(Debug, Clone)]
pub struct Piece {
    pub copy: u64,
    pub asset: u64,
    pub name: String,
    /// Its Heximons value (RAP when it has none).
    pub base: u64,
    /// What the bot counts it at, and why that differs from `base`.
    pub value: u64,
    pub why: String,
}

/// A trade the bot made, waiting to be sent.
#[derive(Debug, Clone)]
pub struct BotTrade {
    pub id: u64,
    pub partner: u64,
    pub name: String,
    pub give: Vec<Piece>,
    pub get: Vec<Piece>,
    /// The inbound trade this answers, for counters.
    pub counter_of: Option<u64>,
    pub kind: TradeKind,
    /// Approved for sending (always, with auto-send on).
    pub approved: bool,
}

impl BotTrade {
    pub fn give_total(&self) -> u64 {
        self.give.iter().map(|piece| piece.value).sum()
    }

    pub fn get_total(&self) -> u64 {
        self.get.iter().map(|piece| piece.value).sum()
    }

    /// What you get back compared with what you give, in percent.
    pub fn gain_pct(&self) -> f64 {
        gain_pct(self.give_total(), self.get_total())
    }
}

pub fn gain_pct(give: u64, get: u64) -> f64 {
    if give == 0 {
        return 0.0;
    }
    (get as f64 - give as f64) / give as f64 * 100.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    Info,
    Sent,
    Good,
    Problem,
}

#[derive(Debug, Clone)]
pub struct LogLine {
    pub at: Instant,
    pub kind: LogKind,
    pub text: String,
}

/// What the screen shows: kept current by the bot, read by the UI.
#[derive(Default)]
pub struct State {
    pub running: bool,
    /// What the bot is doing right now.
    pub phase: String,
    pub next_round: Option<Instant>,
    pub next_send: Option<Instant>,
    pub queue: Vec<BotTrade>,
    pub log: VecDeque<LogLine>,
    pub sent: usize,
    pub countered: usize,
    pub failed: usize,
    pub rounds: u64,
    /// When each trade of the last hour went out.
    sent_times: VecDeque<Instant>,
    /// Inbound trades already looked at this session.
    handled: HashSet<u64>,
    /// Players sent a trade this session.
    partners: HashSet<u64>,
    next_id: u64,
}

impl State {
    fn log(&mut self, kind: LogKind, text: impl Into<String>) {
        self.log.push_front(LogLine { at: Instant::now(), kind, text: text.into() });
        self.log.truncate(LOG_LINES);
    }

    fn push(&mut self, mut trade: BotTrade) {
        self.next_id += 1;
        trade.id = self.next_id;
        self.queue.push(trade);
    }

    /// Trades sent in the last hour.
    pub fn sent_last_hour(&self) -> usize {
        self.sent_times.iter().filter(|at| at.elapsed() < Duration::from_secs(3600)).count()
    }
}

/// Shared between the screen and the bot's task.
#[derive(Default)]
pub struct Shared {
    pub settings: Mutex<BotConfig>,
    pub filters: Mutex<PlayerFilters>,
    /// Players a mass send or the bot went to recently (your "skip recent" filter).
    pub recent: Mutex<HashSet<u64>>,
    pub state: Mutex<State>,
    stop: AtomicBool,
}

impl Shared {
    pub fn new(settings: BotConfig, filters: PlayerFilters) -> Arc<Self> {
        Arc::new(Self { settings: Mutex::new(settings), filters: Mutex::new(filters), ..Default::default() })
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.state.lock().unwrap().phase = "Stopping…".to_string();
    }

    pub fn run_now(&self) {
        self.state.lock().unwrap().next_round = Some(Instant::now());
    }

    pub fn approve(&self, id: u64) {
        if let Some(trade) = self.state.lock().unwrap().queue.iter_mut().find(|trade| trade.id == id) {
            trade.approved = true;
        }
    }

    pub fn approve_all(&self) {
        for trade in &mut self.state.lock().unwrap().queue {
            trade.approved = true;
        }
    }

    pub fn skip(&self, id: u64) {
        let mut state = self.state.lock().unwrap();
        state.queue.retain(|trade| trade.id != id);
    }

    pub fn clear_queue(&self) {
        self.state.lock().unwrap().queue.retain(|trade| trade.approved);
    }

    fn phase(&self, text: &str) {
        self.state.lock().unwrap().phase = text.to_string();
    }

    fn log(&self, kind: LogKind, text: impl Into<String>) {
        self.state.lock().unwrap().log(kind, text);
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

/// Starts the bot's task for the signed-in user. Does nothing if it already runs.
pub fn start(shared: Arc<Shared>, api: Arc<ApiClient>, me: u64, rt: &tokio::runtime::Handle, egui: egui::Context, tx: Sender<Msg>) {
    {
        let mut state = shared.state.lock().unwrap();
        if state.running {
            return;
        }
        state.running = true;
        state.phase = "Starting".to_string();
        state.next_round = Some(Instant::now());
        state.log(LogKind::Info, "Bot started.");
    }
    shared.stop.store(false, Ordering::Relaxed);
    rt.spawn(async move {
        run(&shared, &api, me, &egui, &tx).await;
        let mut state = shared.state.lock().unwrap();
        state.running = false;
        state.phase = "Stopped".to_string();
        state.next_round = None;
        state.log(LogKind::Info, "Bot stopped.");
        egui.request_repaint();
    });
}

async fn run(shared: &Shared, api: &ApiClient, me: u64, egui: &egui::Context, tx: &Sender<Msg>) {
    while !shared.stopped() {
        let settings = shared.settings.lock().unwrap().clone();
        if let Some(trade) = take_sendable(shared, &settings) {
            send(shared, api, me, trade, &settings, tx).await;
            egui.request_repaint();
            continue;
        }
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
        // The countdowns on screen move once a second.
        egui.request_repaint();
    }
}

/// The next approved trade, if the wait and the hourly limit allow one now.
fn take_sendable(shared: &Shared, settings: &BotConfig) -> Option<BotTrade> {
    let mut state = shared.state.lock().unwrap();
    if state.next_send.is_some_and(|at| Instant::now() < at) {
        return None;
    }
    let index = state.queue.iter().position(|trade| trade.approved)?;
    state.sent_times.retain(|at| at.elapsed() < Duration::from_secs(3600));
    if state.sent_times.len() >= settings.per_hour.max(1) as usize {
        state.phase = format!("Hourly limit reached ({} trades)", settings.per_hour.max(1));
        return None;
    }
    Some(state.queue.remove(index))
}

async fn send(shared: &Shared, api: &ApiClient, me: u64, trade: BotTrade, settings: &BotConfig, tx: &Sender<Msg>) {
    if !still_current(shared, api, me, &trade).await {
        return;
    }
    shared.phase(&format!("Sending to {}", trade.name));
    let give: Vec<u64> = trade.give.iter().map(|piece| piece.copy).collect();
    let get: Vec<u64> = trade.get.iter().map(|piece| piece.copy).collect();
    let result = endpoints::send_trade(api, me, trade.partner, &give, &get, 0, 0, trade.counter_of).await;
    let mut state = shared.state.lock().unwrap();
    state.next_send = Some(Instant::now() + Duration::from_secs(settings.gap_secs.max(MASS_MIN_GAP)));
    let what = if trade.counter_of.is_some() { "Countered" } else { "Sent a trade to" };
    match result {
        Ok(()) => {
            state.sent_times.push_back(Instant::now());
            state.partners.insert(trade.partner);
            if trade.counter_of.is_some() {
                state.countered += 1;
            } else {
                state.sent += 1;
            }
            state.log(LogKind::Sent, format!("{what} {}: {}", trade.name, describe(&trade)));
            let _ = tx.send(Msg::BotSent(trade.partner));
        }
        Err(error) => {
            state.failed += 1;
            state.log(LogKind::Problem, format!("{what} {} failed: {error}", trade.name));
        }
    }
    state.phase = "Waiting".to_string();
}

/// Copies in `pieces` that are not in `owned`.
pub fn missing<'a>(pieces: &'a [Piece], owned: &HashSet<u64>) -> Vec<&'a Piece> {
    pieces.iter().filter(|piece| !owned.contains(&piece.copy)).collect()
}

/// Re-reads your inventory (and the partner's) right before a send, so a
/// trade made rounds ago is not sent with items that have moved since.
/// A stale trade is dropped, with any other waiting trade that gives one of
/// the same copies. If your inventory can't be read, the trade goes back to
/// the front of the list and is tried again in 30 seconds.
async fn still_current(shared: &Shared, api: &ApiClient, me: u64, trade: &BotTrade) -> bool {
    shared.phase("Checking your inventory");
    let mine = match endpoints::collectibles_all(api, me).await {
        Ok(items) => items,
        Err(error) => {
            let mut state = shared.state.lock().unwrap();
            state.log(LogKind::Problem, format!("Could not check your inventory before sending to {} ({error}); trying again shortly.", trade.name));
            state.queue.insert(0, trade.clone());
            state.next_send = Some(Instant::now() + Duration::from_secs(30));
            return false;
        }
    };
    let owned: HashSet<u64> = mine.iter().map(|item| item.user_asset_id).collect();
    let gone: Vec<&Piece> = missing(&trade.give, &owned);
    if !gone.is_empty() {
        let names: Vec<&str> = gone.iter().map(|piece| piece.name.as_str()).collect();
        let copies: HashSet<u64> = gone.iter().map(|piece| piece.copy).collect();
        let mut state = shared.state.lock().unwrap();
        let before = state.queue.len();
        state.queue.retain(|other| !other.give.iter().any(|piece| copies.contains(&piece.copy)));
        let also = before - state.queue.len();
        let mut text = format!("Dropped the trade to {}: you no longer have {}.", trade.name, names.join(", "));
        if also > 0 {
            text.push_str(&format!(" {also} other waiting trade(s) with the same item(s) were dropped too."));
        }
        state.log(LogKind::Problem, text);
        return false;
    }
    shared.phase(&format!("Checking {}'s inventory", trade.name));
    // Their side: when it can't be read, send anyway (the site refuses a stale trade).
    if let Ok(theirs) = endpoints::collectibles_all(api, trade.partner).await {
        let owned: HashSet<u64> = theirs.iter().map(|item| item.user_asset_id).collect();
        let gone = missing(&trade.get, &owned);
        if !gone.is_empty() {
            let names: Vec<&str> = gone.iter().map(|piece| piece.name.as_str()).collect();
            shared.log(LogKind::Problem, format!("Dropped the trade to {}: they no longer have {}.", trade.name, names.join(", ")));
            return false;
        }
    }
    true
}

/// "Give A + B (1,200) for C (1,350), +12%".
pub fn describe(trade: &BotTrade) -> String {
    let names = |pieces: &[Piece]| pieces.iter().map(|piece| piece.name.as_str()).collect::<Vec<_>>().join(" + ");
    format!(
        "give {} ({}) for {} ({}), {:+.0}%",
        names(&trade.give),
        crate::ui::money(trade.give_total() as i64),
        names(&trade.get),
        crate::ui::money(trade.get_total() as i64),
        trade.gain_pct()
    )
}

/// One round: read your inventory and trades, counter, then find new trades.
async fn round(shared: &Shared, api: &ApiClient, me: u64, settings: &BotConfig, egui: &egui::Context) {
    let values: Arc<Values> = crate::ui::item_card::current_values();
    if values.items.is_empty() {
        shared.log(LogKind::Problem, "No Heximons values yet; trying again next round.");
        return;
    }
    shared.phase("Reading your inventory");
    egui.request_repaint();
    let mine = match endpoints::collectibles_all(api, me).await {
        Ok(items) => items,
        Err(error) => {
            shared.log(LogKind::Problem, format!("Could not read your inventory: {error}"));
            return;
        }
    };
    let mut busy: HashSet<u64> = match endpoints::trades(api, "outbound", None).await {
        Ok(page) => page.items.iter().map(|trade| trade.partner_id).collect(),
        Err(_) => HashSet::new(),
    };
    {
        let state = shared.state.lock().unwrap();
        busy.extend(state.partners.iter().copied());
        busy.extend(state.queue.iter().map(|trade| trade.partner));
    }
    if settings.counter && !shared.stopped() {
        counters(shared, api, me, settings, &values, egui).await;
    }
    if settings.send_new && !shared.stopped() {
        let waiting = shared.state.lock().unwrap().queue.iter().filter(|trade| !trade.approved).count();
        if waiting >= settings.per_cycle.max(1) * 3 {
            shared.log(LogKind::Info, format!("{waiting} trades are waiting for review; not finding more until you go through them."));
        } else {
            let round_number = shared.state.lock().unwrap().rounds;
            for (kind, kind_settings, count) in round_plan(settings) {
                if shared.stopped() {
                    break;
                }
                // Partners queued by the pass before are busy too.
                busy.extend(shared.state.lock().unwrap().queue.iter().map(|trade| trade.partner));
                new_trades(shared, api, me, settings, &values, &mine, &busy, round_number, kind, kind_settings, count, egui).await;
            }
        }
    }
}

/// How an item sells on the site: sales in the last 30 days, the price
/// change of the last week against the three weeks before, and the
/// cheapest listing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Market {
    pub sales_month: u64,
    pub trend_pct: f64,
    pub best_price: Option<u64>,
}

impl Market {
    pub fn from_data(data: &models::ResaleData, best_price: Option<u64>, today: i64) -> Self {
        let in_days = |from: i64, to: i64| data.points.iter().filter(move |point| point.day > today - from && point.day <= today - to);
        let sales_month = in_days(30, 0).map(|point| point.volume).sum();
        let average = |from: i64, to: i64| {
            let prices: Vec<u64> = in_days(from, to).map(|point| point.price).filter(|price| *price > 0).collect();
            (!prices.is_empty()).then(|| prices.iter().sum::<u64>() as f64 / prices.len() as f64)
        };
        let trend_pct = match (average(7, 0), average(30, 7)) {
            (Some(week), Some(before)) if before > 0.0 => (week - before) / before * 100.0,
            _ => 0.0,
        };
        Self { sales_month, trend_pct, best_price }
    }
}

/// Percent moves at full strength: demand (Terrible, Low, Normal, High).
const DEMAND_PCT: [f64; 4] = [-20.0, -10.0, 0.0, 5.0];

/// Percent move for sales in the last 30 days.
fn sales_pct(sales: u64) -> f64 {
    match sales {
        0 => -20.0,
        1..=2 => -10.0,
        3..=9 => 0.0,
        _ => 5.0,
    }
}

/// What the bot counts an item at: its Heximons value moved by demand, recent
/// sales and half the week's price change (scaled by your strength setting),
/// then capped at its cheapest listing. Returns the value and why it moved.
pub fn adjust(base: u64, demand: Option<u8>, market: Option<&Market>, settings: &BotConfig) -> (u64, String) {
    let mut factor = 1.0;
    let mut why: Vec<String> = Vec::new();
    if settings.adjust && base > 0 {
        let strength = settings.adjust_strength as f64 / 100.0;
        let mut apply = |pct: f64, reason: String| {
            let pct = pct * strength;
            if pct.abs() >= 0.5 {
                factor *= 1.0 + pct / 100.0;
                why.push(format!("{reason} {pct:+.0}%"));
            }
        };
        if let Some(demand) = demand {
            let label = ["terrible demand", "low demand", "normal demand", "high demand"][demand.min(3) as usize];
            apply(DEMAND_PCT[demand.min(3) as usize], label.to_string());
        }
        if let Some(market) = market {
            apply(sales_pct(market.sales_month), format!("{} sales in 30 days", market.sales_month));
            apply(market.trend_pct.clamp(-40.0, 30.0) / 2.0, format!("price {:+.0}% this week", market.trend_pct));
        }
    }
    let mut value = (base as f64 * factor.max(0.1)).round() as u64;
    if let Some(price) = market.and_then(|market| market.best_price) {
        if settings.use_best_price && price < value {
            value = price;
            why.push(format!("cheapest listing {}", crate::ui::money(price as i64)));
        }
    }
    (value, why.join(", "))
}

fn markets() -> &'static Mutex<std::collections::HashMap<u64, (Instant, Market)>> {
    static MARKETS: std::sync::OnceLock<Mutex<std::collections::HashMap<u64, (Instant, Market)>>> = std::sync::OnceLock::new();
    MARKETS.get_or_init(Default::default)
}

fn cached_market(asset: u64) -> Option<Market> {
    markets()
        .lock()
        .unwrap()
        .get(&asset)
        .filter(|(at, _)| at.elapsed() < Duration::from_secs(1800))
        .map(|(_, market)| *market)
}

/// An item's market, read at most every 30 minutes. `None` when the site
/// did not answer (the item then counts at its value, moved by demand only).
async fn market(api: &ApiClient, asset: u64) -> Option<Market> {
    if let Some(market) = cached_market(asset) {
        return Some(market);
    }
    let (data, best) = tokio::join!(endpoints::resale_data(api, asset), endpoints::cheapest_resale(api, asset));
    let data = data.ok()?;
    let today = (now_secs() / 86_400) as i64;
    let market = Market::from_data(&data, best.ok().flatten(), today);
    markets().lock().unwrap().insert(asset, (Instant::now(), market));
    tokio::time::sleep(Duration::from_millis(120)).await;
    Some(market)
}

/// A piece at what the bot counts it for, from what is known so far.
fn piece(values: &Values, settings: &BotConfig, copy: u64, asset: u64, name: &str, rap: u64) -> Piece {
    let base = values.value_or(asset, rap);
    let demand = values.get(asset).and_then(|item| item.demand);
    let (value, why) = adjust(base, demand, cached_market(asset).as_ref(), settings);
    Piece { copy, asset, name: name.to_string(), base, value, why }
}

fn item_piece(values: &Values, settings: &BotConfig, item: &models::Collectible) -> Piece {
    piece(values, settings, item.user_asset_id, item.id, &item.name, item.rap)
}

/// Reads the markets of these pieces and counts them again.
async fn reprice(api: &ApiClient, values: &Values, settings: &BotConfig, pieces: &mut [Piece]) {
    for item in pieces.iter_mut() {
        if market(api, item.asset).await.is_some() {
            let base_rap = item.base;
            *item = piece(values, settings, item.copy, item.asset, &item.name, base_rap);
        }
    }
}

/// The best pick of a player's items at adjusted values: picks on what is
/// known, reads the picked items' markets, and picks again when that moved
/// any of them, until a pick holds.
async fn pick_checked(
    api: &ApiClient,
    settings: &BotConfig,
    values: &Values,
    usable: &[models::Collectible],
    giving: &HashSet<u64>,
    rules: &RangeRules,
) -> Option<(Vec<Piece>, TradeKind)> {
    for _ in 0..5 {
        let candidates: Vec<Piece> = usable
            .iter()
            .filter(|item| !giving.contains(&item.id))
            .map(|item| item_piece(values, settings, item))
            .filter(|piece| piece.value > 0 && piece.value <= rules.max)
            .collect();
        let pairs: Vec<(u64, u64)> = candidates.iter().map(|piece| (piece.copy, piece.value)).collect();
        let (copies, kind) = rules.pick(&pairs)?;
        let mut picked: Vec<Piece> =
            copies.iter().filter_map(|copy| candidates.iter().find(|piece| piece.copy == *copy).cloned()).collect();
        let before: Vec<u64> = picked.iter().map(|piece| piece.value).collect();
        reprice(api, values, settings, &mut picked).await;
        if picked.iter().map(|piece| piece.value).eq(before) {
            return Some((picked, kind));
        }
    }
    None
}

/// The range a trade's other side must land in, for what you give.
pub fn range_for(give: u64, min_gain: i32, max_gain: i32) -> (u64, u64) {
    let low = min_gain.min(max_gain) as i64;
    let high = min_gain.max(max_gain) as i64;
    let at = |percent: i64| ((give as i64 * (100 + percent)) / 100).max(1) as u64;
    (at(low.max(-90)), at(high.max(-90)))
}

/// Inbound trades not yet looked at: good ones are pointed out, the rest
/// get a counter from the sender's inventory when one fits your bar.
async fn counters(shared: &Shared, api: &ApiClient, me: u64, settings: &BotConfig, values: &Values, egui: &egui::Context) {
    shared.phase("Reading inbound trades");
    egui.request_repaint();
    let inbound = match endpoints::trades(api, "inbound", None).await {
        Ok(page) => page.items,
        Err(error) => {
            shared.log(LogKind::Problem, format!("Could not read inbound trades: {error}"));
            return;
        }
    };
    for summary in inbound {
        if shared.stopped() {
            return;
        }
        if !shared.state.lock().unwrap().handled.insert(summary.id) {
            continue;
        }
        let Ok(detail) = endpoints::trade_offers(api, summary.id).await else { continue };
        let (Some(my_offer), Some(their_offer)) = (
            detail.offers.iter().find(|offer| offer.user_id == me),
            detail.offers.iter().find(|offer| offer.user_id != me && offer.user_id != 0),
        ) else {
            continue;
        };
        let name = if their_offer.user_name.is_empty() { summary.partner_name.clone() } else { their_offer.user_name.clone() };
        let to_piece = |asset: &models::TradeAsset| piece(values, settings, asset.user_asset_id, asset.asset_id, &asset.name, asset.rap);
        let mut give: Vec<Piece> = my_offer.assets.iter().map(to_piece).collect();
        let mut get: Vec<Piece> = their_offer.assets.iter().map(to_piece).collect();
        // Both sides at adjusted values.
        reprice(api, values, settings, &mut give).await;
        reprice(api, values, settings, &mut get).await;
        let give_total = give.iter().map(|piece| piece.value).sum::<u64>() + my_offer.robux.unwrap_or(0);
        let get_total = get.iter().map(|piece| piece.value).sum::<u64>() + their_offer.robux.unwrap_or(0);
        if give_total == 0 {
            continue;
        }
        let gain = gain_pct(give_total, get_total);
        if gain >= settings.min_gain as f64 {
            shared.log(LogKind::Good, format!("{name}'s trade is worth accepting ({gain:+.0}% at adjusted values). Open it in Trades."));
            continue;
        }
        if let Some(kept) = give.iter().find(|piece| settings.keep.contains(&piece.asset)) {
            shared.log(LogKind::Info, format!("{name} asks for {}, which you keep; left alone.", kept.name));
            continue;
        }
        if my_offer.robux.is_some() || their_offer.robux.is_some() || give.is_empty() {
            shared.log(LogKind::Info, format!("{name}'s trade has Robux in it ({gain:+.0}%); counter it yourself if you want."));
            continue;
        }
        shared.phase(&format!("Countering {name}"));
        egui.request_repaint();
        let Ok(theirs) = endpoints::collectibles_all(api, their_offer.user_id).await else { continue };
        let giving: HashSet<u64> = give.iter().map(|piece| piece.asset).collect();
        let give_total: u64 = give.iter().map(|piece| piece.value).sum();
        let (min, max) = range_for(give_total, settings.min_gain, settings.max_gain);
        let rules = RangeRules {
            min,
            max,
            my_count: give.len(),
            my_best: give.iter().map(|piece| piece.value).max().unwrap_or(0),
            upgrade: false,
            downgrade: false,
            max_items: TRADE_SIDE_LIMIT,
        };
        match pick_checked(api, settings, values, &theirs, &giving, &rules).await {
            Some((get, kind)) => {
                let trade = BotTrade {
                    id: 0,
                    partner: their_offer.user_id,
                    name: name.clone(),
                    give,
                    get,
                    counter_of: Some(summary.id),
                    kind,
                    approved: settings.auto_send,
                };
                let mut state = shared.state.lock().unwrap();
                state.log(LogKind::Info, format!("Counter for {name} ({gain:+.0}% as sent): {}", describe(&trade)));
                state.push(trade);
            }
            None => shared.log(LogKind::Info, format!("{name}'s trade is {gain:+.0}%; nothing in their inventory makes a counter in your range.")),
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

/// This round's passes: (kind, its settings, trades to find). With both
/// kinds on, the round's trades are split by `upgrade_share`.
pub fn round_plan(settings: &BotConfig) -> Vec<(TradeKind, crate::config::KindSettings, usize)> {
    let total = settings.per_cycle.max(1);
    match (settings.upgrades.on, settings.downgrades.on) {
        (true, false) => vec![(TradeKind::Upgrade, settings.upgrades, total)],
        (false, true) => vec![(TradeKind::Downgrade, settings.downgrades, total)],
        (true, true) => {
            let ups = (total * settings.upgrade_share.min(100) as usize).div_ceil(100);
            let downs = total - ups.min(total);
            let mut plan = Vec::new();
            if ups > 0 {
                plan.push((TradeKind::Upgrade, settings.upgrades, ups));
            }
            if downs > 0 {
                plan.push((TradeKind::Downgrade, settings.downgrades, downs));
            }
            plan
        }
        (false, false) => vec![(
            TradeKind::Even,
            crate::config::KindSettings { on: true, min_gain: settings.min_gain, max_gain: settings.max_gain, max_items: TRADE_SIDE_LIMIT },
            total,
        )],
    }
}

/// Where your items divide: what may be given in a downgrade, and what may be
/// given in an upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAt {
    /// The first `n` items by value.
    Top(usize),
    /// Every item worth at least this much; the rest are upgrade material.
    Value(u64),
}

/// Picks what to give this round. Downgrades give one of your most valuable
/// items (the part above the split), upgrades two up to `max_items`
/// neighbouring items from everything below it, "any" one or two from
/// anywhere. `tradable` is biggest first; the pick rotates with `seed`.
pub fn pick_give(tradable: &[Piece], kind: TradeKind, max_items: usize, split: SplitAt, seed: u64) -> Vec<Piece> {
    let n = tradable.len();
    if n == 0 {
        return Vec::new();
    }
    let cut = match split {
        SplitAt::Top(top) => top.clamp(1, n),
        SplitAt::Value(threshold) => tradable.iter().filter(|piece| piece.value >= threshold).count(),
    };
    match kind {
        TradeKind::Downgrade => {
            // Nothing above the split: pay with an upgrade instead.
            if cut == 0 {
                return Vec::new();
            }
            return vec![tradable[(seed as usize) % cut].clone()];
        }
        TradeKind::Upgrade => {
            // Everything below the split; with a top-item split and too few
            // there, the smallest two or more. A value split is exact: items at
            // or over the value are never given in an upgrade.
            let below = &tradable[cut..];
            let rest = if below.len() >= 2 || matches!(split, SplitAt::Value(_)) {
                below
            } else {
                &tradable[n.saturating_sub(2)..]
            };
            let most = max_items.clamp(2, TRADE_SIDE_LIMIT).min(rest.len());
            if most < 2 || n < 2 {
                return Vec::new();
            }
            let take = 2 + (seed as usize) % (most - 1);
            // Neighbours in value order, so the items are of a similar size.
            let start = (seed as usize / 3) % (rest.len() - take + 1);
            return rest[start..start + take].to_vec();
        }
        TradeKind::Even => {}
    }
    let _ = max_items;
    let take = if n >= 2 && seed % 2 == 1 { 2 } else { 1 };
    let start = ((seed as usize) % n).min(n - take);
    tradable[start..start + take].to_vec()
}

#[allow(clippy::too_many_arguments)]
async fn new_trades(
    shared: &Shared,
    api: &ApiClient,
    me: u64,
    settings: &BotConfig,
    values: &Values,
    mine: &[models::Collectible],
    busy: &HashSet<u64>,
    round_number: u64,
    kind: TradeKind,
    kind_settings: crate::config::KindSettings,
    count: usize,
    egui: &egui::Context,
) {
    let mut tradable: Vec<Piece> = mine
        .iter()
        .filter(|item| !settings.keep.contains(&item.id))
        .map(|item| item_piece(values, settings, item))
        .filter(|piece| piece.value > 0)
        .collect();
    tradable.sort_by(|a, b| b.value.cmp(&a.value));
    if tradable.is_empty() {
        shared.log(LogKind::Problem, "Nothing to trade: every valued item you own is on the keep list.");
        return;
    }
    let seed = round_number.wrapping_mul(7).wrapping_add(now_secs() / 60);
    let split = match settings.split {
        crate::config::Split::Top => SplitAt::Top(settings.top_items),
        crate::config::Split::Value => SplitAt::Value(settings.upgrade_to),
    };
    let mut give = pick_give(&tradable, kind, kind_settings.max_items, split, seed);
    if give.is_empty() {
        shared.log(LogKind::Problem, "Upgrades need at least two items you are willing to trade.");
        return;
    }
    reprice(api, values, settings, &mut give).await;
    let give_total: u64 = give.iter().map(|piece| piece.value).sum();
    let (min, max) = range_for(give_total, kind_settings.min_gain, kind_settings.max_gain);
    let rules = RangeRules {
        min,
        max,
        my_count: give.len(),
        my_best: give.iter().map(|piece| piece.value).max().unwrap_or(0),
        upgrade: kind == TradeKind::Upgrade,
        downgrade: kind == TradeKind::Downgrade,
        // Downgrades cap what comes back; upgrades get fewer than they give anyway.
        max_items: if kind == TradeKind::Downgrade { kind_settings.max_items.clamp(2, TRADE_SIDE_LIMIT) } else { TRADE_SIDE_LIMIT },
    };
    let giving: HashSet<u64> = give.iter().map(|piece| piece.asset).collect();
    let my_best = rules.my_best;
    let label = match kind {
        TradeKind::Upgrade => "an upgrade",
        TradeKind::Downgrade => "a downgrade",
        TradeKind::Even => "a trade",
    };
    let names: Vec<&str> = give.iter().map(|piece| piece.name.as_str()).collect();
    shared.log(
        LogKind::Info,
        format!(
            "Looking for {label} for {} ({} adjusted), wanting {} to {} back.",
            names.join(" + "),
            crate::ui::money(give_total as i64),
            crate::ui::money(min as i64),
            crate::ui::money(max as i64)
        ),
    );
    shared.phase("Finding players");
    egui.request_repaint();

    // Valued items that could make up (part of) the range, biggest first; the
    // start rotates so rounds reach different owners.
    let mut seeds: Vec<(u64, u64)> = values
        .items
        .iter()
        .filter(|(asset, item)| item.value > 0 && item.value <= max && !giving.contains(asset))
        .filter(|(_, item)| match kind {
            // The item you would get: bigger than anything you give.
            TradeKind::Upgrade => item.value > my_best,
            // Pieces of the return: smaller than what you give.
            TradeKind::Downgrade => item.value < my_best && item.value >= (min / 6).max(1),
            TradeKind::Even => item.value >= (min / 4).max(1),
        })
        .map(|(asset, item)| (*asset, item.value))
        .collect();
    seeds.sort_by(|a, b| b.1.cmp(&a.1));
    if !seeds.is_empty() {
        let shift = (seed as usize) % seeds.len().min(24);
        seeds.rotate_left(shift);
    }
    seeds.truncate(12);
    let mut found_players: Vec<Found> = Vec::new();
    for (asset, _) in &seeds {
        if shared.stopped() {
            return;
        }
        if let Ok(owners) = endpoints::asset_owners(api, *asset, 1).await {
            for owner in owners {
                if owner.user_id != me && owner.user_id > 1 && !busy.contains(&owner.user_id)
                    && !found_players.iter().any(|(id, ..)| *id == owner.user_id)
                {
                    found_players.push((owner.user_id, owner.name, owner.updated));
                }
            }
        }
        if found_players.len() >= 150 {
            break;
        }
    }
    let filters = *shared.filters.lock().unwrap();
    let recent = shared.recent.lock().unwrap().clone();
    let ordered = order_players(found_players, &filters, &recent, seed);
    shared.phase("Checking players");
    egui.request_repaint();
    let players = check_players(api, ordered, &filters, 40).await;
    let mut made = 0;
    for (index, (user_id, name, _)) in players.into_iter().take(40).enumerate() {
        if shared.stopped() || made >= count.max(1) {
            break;
        }
        shared.phase(&format!("Reading inventories ({index} checked, {made} found)"));
        egui.request_repaint();
        let Ok(items) = endpoints::collectibles_all(api, user_id).await else { continue };
        if let Some((get, kind)) = pick_checked(api, settings, values, &items, &giving, &rules).await {
            let trade = BotTrade { id: 0, partner: user_id, name, give: give.clone(), get, counter_of: None, kind, approved: settings.auto_send };
            shared.state.lock().unwrap().push(trade);
            made += 1;
            egui.request_repaint();
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let what = if settings.auto_send { "queued to send" } else { "waiting for your review" };
    shared.log(LogKind::Info, format!("Found {made} trade(s), {what}."));
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|elapsed| elapsed.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pieces(values: &[u64]) -> Vec<Piece> {
        values
            .iter()
            .enumerate()
            .map(|(index, value)| Piece { copy: index as u64 + 1, asset: index as u64 + 100, name: String::new(), base: *value, value: *value, why: String::new() })
            .collect()
    }

    #[test]
    fn finds_items_that_moved() {
        let give = pieces(&[500, 300, 200]);
        let owned: HashSet<u64> = [1, 3].into_iter().collect();
        let gone = missing(&give, &owned);
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].copy, 2);
        assert!(missing(&give, &[1, 2, 3].into_iter().collect()).is_empty());
    }

    #[test]
    fn ranges_follow_the_gain_settings() {
        assert_eq!(range_for(1000, 5, 20), (1050, 1200));
        // Upgrades may overpay: a negative gain asks for less back.
        assert_eq!(range_for(1000, -15, 0), (850, 1000));
        // Swapped bounds are put in order.
        assert_eq!(range_for(1000, 20, 5), (1050, 1200));
    }

    #[test]
    fn values_move_with_demand_and_market() {
        let settings = BotConfig::default();
        let steady = Market { sales_month: 5, trend_pct: 0.0, best_price: None };
        // Normal demand, steady sales: unchanged.
        assert_eq!(adjust(1000, Some(2), Some(&steady), &settings).0, 1000);
        // Low demand -10%, no sales -20%: 1000 * 0.9 * 0.8.
        let dead = Market { sales_month: 0, ..steady };
        assert_eq!(adjust(1000, Some(1), Some(&dead), &settings).0, 720);
        // Price up 20% this week counts half: +10%.
        let rising = Market { trend_pct: 20.0, ..steady };
        assert_eq!(adjust(1000, Some(2), Some(&rising), &settings).0, 1100);
        // A cheaper listing caps it.
        let listed = Market { best_price: Some(950), ..rising };
        let (value, why) = adjust(1000, Some(2), Some(&listed), &settings);
        assert_eq!(value, 950);
        assert!(why.contains("cheapest listing"));
        // Half strength halves the moves; off leaves only the listing cap.
        let half = BotConfig { adjust_strength: 50, ..settings.clone() };
        assert_eq!(adjust(1000, Some(1), Some(&steady), &half).0, 950);
        let off = BotConfig { adjust: false, use_best_price: false, ..settings };
        assert_eq!(adjust(1000, Some(0), Some(&dead), &off).0, 1000);
    }

    #[test]
    fn market_reads_sales_and_trend() {
        let point = |day: i64, price: u64, volume: u64| models::ResalePoint { day, price, volume };
        let data = models::ResaleData {
            points: vec![point(60, 500, 9), point(80, 1000, 2), point(90, 1000, 1), point(97, 1200, 3), point(99, 1200, 1)],
            ..Default::default()
        };
        let market = Market::from_data(&data, Some(1100), 100);
        // Day 60 is older than 30 days.
        assert_eq!(market.sales_month, 7);
        assert!((market.trend_pct - 20.0).abs() < 0.01);
    }

    #[test]
    fn downgrades_top_items_and_upgrades_the_rest() {
        let tradable = pieces(&[9000, 8000, 7000, 3000, 2500, 2000, 900, 800, 700]);
        for seed in 0..30 {
            let down = pick_give(&tradable, TradeKind::Downgrade, 4, SplitAt::Top(2), seed);
            assert!(down[0].value >= 8000, "seed {seed} gave {}", down[0].value);
            let up = pick_give(&tradable, TradeKind::Upgrade, 3, SplitAt::Top(2), seed);
            assert!(up.len() >= 2 && up.iter().all(|piece| piece.value <= 7000), "seed {seed}");
        }
        // Upgrades reach the whole rest, not just the bottom.
        assert!((0..30).any(|seed| pick_give(&tradable, TradeKind::Upgrade, 2, SplitAt::Top(2), seed)[0].value == 7000));
    }

    #[test]
    fn value_split_upgrades_under_the_threshold() {
        let tradable = pieces(&[9000, 8000, 3000, 2500, 2000, 900]);
        for seed in 0..30 {
            let down = pick_give(&tradable, TradeKind::Downgrade, 3, SplitAt::Value(4000), seed);
            assert_eq!(down.len(), 1);
            assert!(down[0].value >= 4000, "seed {seed} gave {}", down[0].value);
            let up = pick_give(&tradable, TradeKind::Upgrade, 3, SplitAt::Value(4000), seed);
            assert!(up.len() >= 2 && up.iter().all(|piece| piece.value < 4000), "seed {seed}");
        }
        // Nothing over the value: no downgrades. Nothing under it: no upgrades.
        assert!(pick_give(&tradable, TradeKind::Downgrade, 3, SplitAt::Value(100_000), 0).is_empty());
        assert!(pick_give(&tradable, TradeKind::Upgrade, 3, SplitAt::Value(1), 0).is_empty());
        // Exactly at the value counts as downgrade material, never upgrade material.
        let at = pick_give(&tradable, TradeKind::Downgrade, 3, SplitAt::Value(8000), 0);
        assert_eq!(at.len(), 1);
        assert!(at[0].value >= 8000);
        for seed in 0..30 {
            assert!(pick_give(&tradable, TradeKind::Upgrade, 3, SplitAt::Value(8000), seed)
                .iter()
                .all(|piece| piece.value < 8000));
        }
    }

    #[test]
    fn give_sets_match_the_kind() {
        let tradable = pieces(&[9000, 5000, 3000, 2000, 1500, 1000, 800]);
        assert_eq!(pick_give(&tradable, TradeKind::Downgrade, 4, SplitAt::Top(2), 3).len(), 1);
        assert_eq!(pick_give(&tradable, TradeKind::Upgrade, 3, SplitAt::Top(2), 1).len(), 3);
        assert_eq!(pick_give(&tradable, TradeKind::Upgrade, 3, SplitAt::Top(2), 2).len(), 2);
        for seed in 0..40 {
            assert_eq!(pick_give(&tradable, TradeKind::Upgrade, 2, SplitAt::Top(2), seed).len(), 2);
        }
        // Too few below the top: the smallest two.
        assert_eq!(pick_give(&pieces(&[900, 500, 400]), TradeKind::Upgrade, 3, SplitAt::Top(2), 0).len(), 2);
        assert!(pick_give(&pieces(&[500]), TradeKind::Upgrade, 3, SplitAt::Top(2), 0).is_empty());
    }

    #[test]
    fn rounds_split_by_upgrade_share() {
        let mut settings = BotConfig { per_cycle: 5, ..BotConfig::default() };
        settings.upgrades.on = true;
        settings.downgrades.on = true;
        let counts = |settings: &BotConfig| round_plan(settings).iter().map(|(kind, _, count)| (*kind, *count)).collect::<Vec<_>>();
        assert_eq!(counts(&settings), vec![(TradeKind::Upgrade, 3), (TradeKind::Downgrade, 2)]);
        settings.upgrade_share = 100;
        assert_eq!(counts(&settings), vec![(TradeKind::Upgrade, 5)]);
        settings.upgrade_share = 0;
        assert_eq!(counts(&settings), vec![(TradeKind::Downgrade, 5)]);
    }
}
