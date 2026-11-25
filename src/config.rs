//! Local settings. The session cookie is the only sensitive value and it lives
//! here, outside the repository, in the OS config directory.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const DEFAULT_ORIGIN: &str = "https://hexium.wtf";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_origin")]
    pub origin: String,
    #[serde(default)]
    pub session_cookie: Option<String>,
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Accent colour key, one of `ui::ACCENTS`.
    #[serde(default = "default_accent")]
    pub accent: String,
    #[serde(default = "default_width")]
    pub window_width: f32,
    #[serde(default = "default_height")]
    pub window_height: f32,
    /// Tell the other person in a chat when you are typing, as the site does.
    #[serde(default = "default_true")]
    pub share_typing: bool,
    /// Windows notifications for chat messages you are not looking at.
    #[serde(default = "default_true")]
    pub chat_notifications: bool,
    /// Windows notifications for new trades, friend requests and messages.
    #[serde(default = "default_true")]
    pub activity_notifications: bool,
    /// Limiteds to watch, each with the price to be told about.
    #[serde(default)]
    pub watchlist: Vec<WatchItem>,
    /// Newest Heximons value change already seen (seconds); 0 = not started.
    #[serde(default)]
    pub last_change_seen: u64,
    /// Deal notifications (off until switched on).
    #[serde(default)]
    pub deal_alerts: bool,
    #[serde(default = "default_deal_alert_min")]
    pub deal_alert_min: f32,
    #[serde(default)]
    pub deal_alert_staff_only: bool,
    #[serde(default)]
    pub deal_alert_high_demand: bool,
    /// Deals already notified: (asset, price). Kept short.
    #[serde(default)]
    pub seen_deals: Vec<(u64, u64)>,
    /// Your own item values: they replace Heximons' or add missing ones.
    #[serde(default)]
    pub value_overrides: Vec<ValueOverride>,
    /// Who the trade helpers send to (owner lists, mass send, value range).
    #[serde(default)]
    pub player_filters: PlayerFilters,
    /// People mass sends went to: (user id, when, seconds since 1970).
    #[serde(default)]
    pub recent_targets: Vec<(u64, u64)>,
    /// Most owners one mass send goes to (1 to 50).
    #[serde(default = "default_mass_cap")]
    pub mass_cap: usize,
    /// Seconds between trades in a mass send (10 to 600).
    #[serde(default = "default_mass_gap")]
    pub mass_gap_secs: u64,
    /// Text size (1.0 = normal).
    #[serde(default = "default_scale")]
    pub ui_scale: f32,
    /// Tighter spacing.
    #[serde(default)]
    pub compact: bool,
    /// Closing the window hides it to the tray instead of quitting.
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    /// The exe the Start-menu shortcut was last written for.
    #[serde(default)]
    pub shortcut_exe: Option<String>,
    /// The trade bot.
    #[serde(default)]
    pub bot: BotConfig,
    /// The resale bot.
    #[serde(default)]
    pub resale: ResaleConfig,
    /// Install new releases from GitHub on their own.
    #[serde(default = "default_true")]
    pub auto_update: bool,
    /// Roblox .ROBLOSECURITY cookie — used to download clothing templates.
    #[serde(default)]
    pub roblox_cookie: Option<String>,
}

fn default_deal_alert_min() -> f32 {
    30.0
}

fn default_mass_cap() -> usize {
    10
}

fn default_mass_gap() -> u64 {
    30
}

fn default_scale() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}

fn default_width() -> f32 {
    1300.0
}

fn default_height() -> f32 {
    850.0
}

fn default_origin() -> String {
    DEFAULT_ORIGIN.to_string()
}

fn default_theme() -> String {
    "midnight".to_string()
}

fn default_accent() -> String {
    "coral".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            origin: default_origin(),
            session_cookie: None,
            theme: default_theme(),
            accent: default_accent(),
            window_width: default_width(),
            window_height: default_height(),
            share_typing: true,
            chat_notifications: true,
            close_to_tray: true,
            activity_notifications: true,
            watchlist: Vec::new(),
            last_change_seen: 0,
            deal_alerts: false,
            deal_alert_min: default_deal_alert_min(),
            deal_alert_staff_only: false,
            deal_alert_high_demand: false,
            seen_deals: Vec::new(),
            ui_scale: default_scale(),
            mass_cap: default_mass_cap(),
            player_filters: PlayerFilters::default(),
            recent_targets: Vec::new(),
            mass_gap_secs: default_mass_gap(),
            value_overrides: Vec::new(),
            compact: false,
            shortcut_exe: None,
            bot: BotConfig::default(),
            resale: ResaleConfig::default(),
            auto_update: true,
            roblox_cookie: None,
        }
    }
}

pub fn config_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("hexium-client");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config").join("hexium-client");
    }
    PathBuf::from(".hexium-client")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

pub fn load() -> Config {
    let mut config = std::fs::read_to_string(config_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<Config>(&raw).ok())
        .unwrap_or_default();

    if config.origin.trim().is_empty() {
        config.origin = default_origin();
    }
    if config.session_cookie.is_none() {
        if let Ok(from_env) = std::env::var("HEXIUM_COOKIE") {
            let cookie = Config::normalize_cookie(&from_env);
            if !cookie.is_empty() {
                config.session_cookie = Some(cookie);
            }
        }
    }
    config
}

impl Config {
    pub fn save(&self) {
        let directory = config_dir();
        let _ = std::fs::create_dir_all(&directory);
        if let Ok(raw) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(config_path(), raw);
        }
    }

    /// Accepts the raw value, a `name=value` pair, or a pasted Set-Cookie line.
    pub fn normalize_cookie(input: &str) -> String {
        let mut value = input.trim().trim_matches('"').trim();
        if let Some(rest) = value.strip_prefix(".ROBLOSECURITY=") {
            value = rest;
        }
        if let Some(rest) = value.strip_prefix("ROBLOSECURITY=") {
            value = rest;
        }
        if let Some((head, _)) = value.split_once(';') {
            value = head;
        }
        value.trim().to_string()
    }

    pub fn masked_session(&self) -> Option<String> {
        self.session_cookie.as_ref().map(|cookie| {
            let tail: String = cookie.chars().rev().take(4).collect();
            let tail: String = tail.chars().rev().collect();
            format!("stored · {} chars · …{tail}", cookie.chars().count())
        })
    }

    pub fn masked_roblox_cookie(&self) -> Option<String> {
        self.roblox_cookie.as_ref().map(|cookie| {
            let tail: String = cookie.chars().rev().take(4).collect();
            let tail: String = tail.chars().rev().collect();
            format!("stored · {} chars · …{tail}", cookie.chars().count())
        })
    }
}

/// A watched limited: alert when its cheapest reseller is at or below `target`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchItem {
    pub asset_id: u64,
    pub name: String,
    pub target: u64,
}

/// A value you set for an item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueOverride {
    pub asset_id: u64,
    pub name: String,
    pub value: u64,
}

/// Filters on the players the trade helpers pick. 0 turns a number off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerFilters {
    /// Account at least this many days old.
    pub min_account_days: u32,
    /// Has held the item at least / at most this many days.
    pub min_held_days: u32,
    pub max_held_days: u32,
    /// Seen online within this many days.
    pub active_within_days: u32,
    /// Leave out people a mass send went to within this many days.
    pub skip_recent_days: u32,
    /// 0 newest holders first, 1 longest holders first, 2 random.
    pub order: u8,
    /// Only players with Builders Club (any tier); trades need it on both sides.
    pub builders_club_only: bool,
}

impl Default for PlayerFilters {
    fn default() -> Self {
        Self {
            min_account_days: 0,
            min_held_days: 0,
            max_held_days: 0,
            active_within_days: 0,
            skip_recent_days: 7,
            order: 2,
            builders_club_only: true,
        }
    }
}

/// Settings for one kind of new trade (upgrades or downgrades).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KindSettings {
    pub on: bool,
    /// What you get back, compared with what you give, in percent.
    pub min_gain: i32,
    pub max_gain: i32,
    /// Upgrades: most items you give. Downgrades: most items you get back.
    pub max_items: usize,
}

impl Default for KindSettings {
    fn default() -> Self {
        Self { on: false, min_gain: 0, max_gain: 20, max_items: 4 }
    }
}

/// How the trade bot divides your items between upgrades and downgrades.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Split {
    /// Downgrade your `top_items` most valuable items; upgrade everything below.
    #[default]
    Top,
    /// Upgrade items worth under `upgrade_to`; downgrade everything at or over it.
    Value,
}

fn default_upgrade_to() -> u64 {
    1_000
}

/// The trade bot's settings (Trade bot screen).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BotConfig {
    /// Make new trades from your inventory and send them.
    pub send_new: bool,
    /// Answer inbound trades below your bar with a counter.
    pub counter: bool,
    /// Only answer inbound trades: counters are built and queued for you, and
    /// no new trades are found or sent on the bot's own.
    #[serde(default)]
    pub counter_only: bool,
    pub upgrades: KindSettings,
    pub downgrades: KindSettings,
    /// Downgrades give from your this-many most valuable items; upgrades
    /// give from everything below them.
    pub top_items: usize,
    /// How upgrades and downgrades split your items: by position (`Top`) or
    /// by what each item is worth (`Value`).
    #[serde(default)]
    pub split: Split,
    /// `Split::Value`: items worth less than this are upgraded, the rest are
    /// downgraded. At or over it counts as downgrade material.
    #[serde(default = "default_upgrade_to")]
    pub upgrade_to: u64,
    /// With both kinds on: the share of each round's new trades that are
    /// upgrades, in percent (the rest are downgrades).
    pub upgrade_share: u32,
    /// Other trades (when neither kind is on) and counters: what you get
    /// back compared with what you give, in percent.
    pub min_gain: i32,
    pub max_gain: i32,
    /// Send without asking; off, trades wait in the review list.
    pub auto_send: bool,
    pub per_hour: u32,
    pub gap_secs: u64,
    /// Minutes between rounds of finding trades and reading inbound ones.
    pub cycle_minutes: u64,
    /// New trades found per round.
    pub per_cycle: usize,
    /// Move item values up or down for demand, recent sales and the
    /// week's price change.
    pub adjust: bool,
    /// How strongly, in percent (100 = as described on screen).
    pub adjust_strength: u32,
    /// Count an item at its cheapest listing when that is below its value.
    pub use_best_price: bool,
    /// Items (asset ids) the bot never gives away.
    pub keep: Vec<u64>,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            send_new: true,
            counter: true,
            counter_only: false,
            upgrades: KindSettings { on: false, min_gain: -10, max_gain: 10, max_items: 3 },
            downgrades: KindSettings { on: false, min_gain: 5, max_gain: 25, max_items: 4 },
            top_items: 2,
            split: Split::Top,
            upgrade_to: default_upgrade_to(),
            upgrade_share: 50,
            min_gain: 5,
            max_gain: 20,
            auto_send: false,
            per_hour: 20,
            gap_secs: 30,
            cycle_minutes: 10,
            per_cycle: 5,
            adjust: true,
            adjust_strength: 100,
            use_best_price: true,
            keep: Vec::new(),
        }
    }
}

/// The resale bot's settings (Resale screen).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResaleConfig {
    /// How far below the cheapest other listing to list, in R$.
    pub undercut: u64,
    /// Never list below this price. 0 would take a copy off sale, so 1 is the floor.
    pub min_price: u64,
    /// Minutes between rounds.
    pub cycle_minutes: u64,
    /// Seconds between two price changes, so writes are not a burst.
    pub gap_secs: u64,
    /// Leave out the items on the trade bot's keep list.
    pub skip_kept: bool,
}

impl Default for ResaleConfig {
    fn default() -> Self {
        Self { undercut: 1, min_price: 1, cycle_minutes: 10, gap_secs: 5, skip_kept: true }
    }
}
