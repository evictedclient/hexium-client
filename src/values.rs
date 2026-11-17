//! Item values from heximons.lol, a community values site for Hexium limiteds.
//!
//! Its catalog page embeds every tracked limited as `var item_details = {...}`,
//! plain JSON keyed `"1:<assetId>"` with a fixed field order. The asset id is the
//! same as on hexium.wtf. The client reads that page now and then (at most every
//! 15 minutes, cached on disk), sends no cookie or account data there, and says
//! who it is in its user agent. Heximons' robots.txt allows this
//! (`User-agent: *`, `Allow: /`, `use=reference`).

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SOURCE: &str = "https://heximons.lol/catalog";
const USER_AGENT: &str = "hexium-client (desktop app; reads values for its user)";
pub const REFRESH: Duration = Duration::from_secs(15 * 60);

/// Field positions in an `item_details` entry, matched against the page's own
/// labelled objects across all items.
mod field {
    pub const NAME: usize = 0;
    pub const RAP: usize = 8;
    pub const ACRONYM: usize = 15;
    pub const STAFF_VALUE: usize = 16;
    pub const DEMAND: usize = 17;
    pub const TREND: usize = 18;
    pub const RARE: usize = 21;
    pub const VALUE: usize = 22;
    pub const ID: usize = 25;
    /// Cheapest listing Heximons last saw (matched against the page's labelled
    /// objects: all 427 items with sellers agree; null when nobody sells).
    pub const BEST_PRICE: usize = 5;
    /// Hexium CDN thumbnail.
    pub const THUMB: usize = 23;
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ItemValue {
    pub name: String,
    pub acronym: Option<String>,
    pub rap: u64,
    /// The staff value when there is one, otherwise RAP (what Heximons shows).
    pub value: u64,
    /// Set only when staff have valued the item.
    pub staff_value: Option<u64>,
    /// 0 Terrible, 1 Low, 2 Normal, 3 High.
    pub demand: Option<u8>,
    /// 1 Unstable, 2 Stable, 3 Raising, 4 Fluctuating.
    pub trend: Option<u8>,
    pub rare: bool,
    /// Cheapest listing when Heximons last looked.
    #[serde(default)]
    pub best_price: Option<u64>,
    /// Thumbnail on hexium.wtf's image server.
    #[serde(default)]
    pub thumb: Option<String>,
}

impl ItemValue {
    pub fn demand_label(&self) -> Option<&'static str> {
        match self.demand? {
            0 => Some("Terrible"),
            1 => Some("Low"),
            2 => Some("Normal"),
            3 => Some("High"),
            _ => None,
        }
    }

    pub fn trend_label(&self) -> Option<&'static str> {
        match self.trend? {
            1 => Some("Unstable"),
            2 => Some("Stable"),
            3 => Some("Raising"),
            4 => Some("Fluctuating"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Values {
    /// By Hexium asset id.
    pub items: HashMap<u64, ItemValue>,
    /// Seconds since 1970 when this was read from Heximons.
    pub fetched_at: u64,
}

impl Values {
    pub fn get(&self, asset_id: u64) -> Option<&ItemValue> {
        self.items.get(&asset_id)
    }

    /// The value to count an item at: Heximons' value, else the RAP given.
    pub fn value_or(&self, asset_id: u64, rap: u64) -> u64 {
        self.get(asset_id).map(|item| item.value).filter(|value| *value > 0).unwrap_or(rap)
    }

    pub fn is_stale(&self) -> bool {
        now_secs().saturating_sub(self.fetched_at) >= REFRESH.as_secs()
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_secs()).unwrap_or(0)
}

fn number(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64().or_else(|| number.as_f64().map(|float| float.max(0.0) as u64)),
        Value::String(text) => text.replace(',', "").trim().parse().ok(),
        _ => None,
    }
}

/// Reads the `item_details` object out of the catalog page.
pub fn parse(html: &str) -> Result<HashMap<u64, ItemValue>, String> {
    let marker = "item_details";
    let at = html.find(marker).ok_or("the Heximons page has no item list")?;
    let start = html[at..].find('{').map(|offset| at + offset).ok_or("no item list object")?;
    // The object is followed by `;` and the end of the statement.
    let end = html[start..].find("};").map(|offset| start + offset + 1).ok_or("item list never ends")?;
    let parsed: Value = serde_json::from_str(&html[start..end]).map_err(|error| format!("item list is not JSON: {error}"))?;
    let object = parsed.as_object().ok_or("item list is not an object")?;

    let mut items = HashMap::new();
    for (key, entry) in object {
        let Some(fields) = entry.as_array() else { continue };
        let at = |index: usize| fields.get(index).unwrap_or(&Value::Null);
        let id = number(at(field::ID)).or_else(|| key.split(':').nth(1).and_then(|id| id.parse().ok()));
        let Some(id) = id.filter(|id| *id != 0) else { continue };
        let rap = number(at(field::RAP)).unwrap_or(0);
        let staff_value = number(at(field::STAFF_VALUE)).filter(|value| *value > 0);
        items.insert(
            id,
            ItemValue {
                name: at(field::NAME).as_str().unwrap_or_default().to_string(),
                acronym: at(field::ACRONYM).as_str().filter(|text| !text.is_empty()).map(str::to_string),
                rap,
                value: staff_value.or_else(|| number(at(field::VALUE))).unwrap_or(rap),
                staff_value,
                demand: number(at(field::DEMAND)).map(|demand| demand as u8),
                trend: number(at(field::TREND)).map(|trend| trend as u8),
                rare: number(at(field::RARE)) == Some(1),
                best_price: number(at(field::BEST_PRICE)).filter(|price| *price > 0),
                thumb: at(field::THUMB).as_str().filter(|url| url.starts_with("https://")).map(str::to_string),
            },
        );
    }
    if items.is_empty() {
        return Err("the Heximons item list was empty".to_string());
    }
    Ok(items)
}

pub async fn fetch() -> Result<Values, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(SOURCE)
        .send()
        .await
        .map_err(|error| format!("could not reach heximons.lol: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("heximons.lol answered HTTP {}", response.status().as_u16()));
    }
    let html = response.text().await.map_err(|error| error.to_string())?;
    Ok(Values { items: parse(&html)?, fetched_at: now_secs() })
}

fn cache_path() -> std::path::PathBuf {
    crate::config::config_dir().join("heximons-values.json")
}

pub fn load_cache() -> Option<Values> {
    let text = std::fs::read_to_string(cache_path()).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_cache(values: &Values) {
    if let Ok(text) = serde_json::to_string(values) {
        let _ = std::fs::create_dir_all(crate::config::config_dir());
        let _ = std::fs::write(cache_path(), text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live page's shape (values made up): one valued item, one not.
    #[test]
    fn parses_the_embedded_item_list() {
        let html = r#"<html><script>var item_details = {
            "1:907":["): Green Sidewinder","asset:8",5,1784339522,1789852604,999999,4,7,2560,42,42,999999,15,42,9,"GSW",5500,2,2,null,null,null,5500,"https://x/a.png",1,907],
            "1:225":["Bighead","asset:8",5,1784339522,1789852604,200,4,7,111,181,178,200,15,183,9,null,null,null,null,null,null,1,111,"https://x/b.png",1,225]
        };
        var other = 1;</script></html>"#;
        let items = parse(html).unwrap();
        let gsw = &items[&907];
        assert_eq!((gsw.rap, gsw.value, gsw.staff_value), (2560, 5500, Some(5500)));
        assert_eq!((gsw.demand_label(), gsw.trend_label()), (Some("Normal"), Some("Stable")));
        assert_eq!(gsw.acronym.as_deref(), Some("GSW"));
        let bighead = &items[&225];
        assert_eq!((bighead.value, bighead.staff_value, bighead.demand), (111, None, None));
        assert!(bighead.rare);
        assert!(parse("<html>nothing</html>").is_err());
    }

    #[test]
    fn value_falls_back_to_rap() {
        let mut values = Values::default();
        values.items.insert(1, ItemValue { value: 900, ..Default::default() });
        assert_eq!(values.value_or(1, 100), 900);
        assert_eq!(values.value_or(2, 100), 100);
    }
}

/// One day's total for your collectibles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct HistoryPoint {
    /// Days since 1970-01-01 (UTC).
    pub day: i64,
    pub rap: u64,
    pub value: u64,
}

fn history_path() -> std::path::PathBuf {
    crate::config::config_dir().join("value-history.json")
}

pub fn load_history() -> Vec<HistoryPoint> {
    std::fs::read_to_string(history_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Records today's totals (replacing an earlier reading from today) and saves.
pub fn record_history(history: &mut Vec<HistoryPoint>, rap: u64, value: u64) {
    let day = (now_secs() / 86_400) as i64;
    match history.iter_mut().find(|point| point.day == day) {
        Some(point) => {
            point.rap = rap;
            point.value = value;
        }
        None => history.push(HistoryPoint { day, rap, value }),
    }
    history.sort_by_key(|point| point.day);
    if let Ok(text) = serde_json::to_string(history) {
        let _ = std::fs::create_dir_all(crate::config::config_dir());
        let _ = std::fs::write(history_path(), text);
    }
}

// ---------------------------------------------------------------------------
// Deals: listings well below their value
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Deal {
    pub asset_id: u64,
    pub name: String,
    pub price: u64,
    pub value: u64,
    pub staff_valued: bool,
    /// How far below value, in percent.
    pub discount: f32,
}

/// Items whose best price is at least `min_discount` percent below their value
/// (the staff value, otherwise RAP), biggest discount first.
pub fn deals(values: &Values, min_discount: f32, staff_only: bool) -> Vec<Deal> {
    let mut found: Vec<Deal> = values
        .items
        .iter()
        .filter_map(|(asset_id, item)| {
            let price = item.best_price?;
            if item.value == 0 || price >= item.value || (staff_only && item.staff_value.is_none()) {
                return None;
            }
            let discount = (item.value - price) as f32 / item.value as f32 * 100.0;
            (discount >= min_discount).then(|| Deal {
                asset_id: *asset_id,
                name: item.name.clone(),
                price,
                value: item.value,
                staff_valued: item.staff_value.is_some(),
                discount,
            })
        })
        .collect();
    found.sort_by(|a, b| b.discount.total_cmp(&a.discount).then(a.name.cmp(&b.name)));
    found
}

// ---------------------------------------------------------------------------
// A player's value history (heximons.lol/player/{id})
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayerPoint {
    /// Seconds since 1970.
    pub time: u64,
    pub value: u64,
    pub rap: u64,
    pub collectibles: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerStats {
    pub rank: Option<u64>,
    /// Oldest first.
    pub points: Vec<PlayerPoint>,
}

impl PlayerStats {
    pub fn latest(&self) -> Option<PlayerPoint> {
        self.points.last().copied()
    }
}

/// The JSON object assigned to `var <name> = {...};` in a page.
fn embedded(html: &str, name: &str) -> Option<Value> {
    let at = html.find(&format!("var {name}"))?;
    let start = at + html[at..].find('{')?;
    let end = start + html[start..].find("};")? + 1;
    serde_json::from_str(&html[start..end]).ok()
}

/// Reads `chart_data` (parallel `nominal_scan_time`, `value`, `rap`,
/// `num_limiteds` arrays) and the rank from `player_details_data`.
pub fn parse_player(html: &str) -> Result<PlayerStats, String> {
    let chart = embedded(html, "chart_data").ok_or("the Heximons player page has no value history")?;
    let series = |key: &str| -> Vec<u64> {
        chart
            .get(key)
            .and_then(Value::as_array)
            .map(|items| items.iter().map(|item| number(item).unwrap_or(0)).collect())
            .unwrap_or_default()
    };
    let times = series("nominal_scan_time");
    let (value, rap, count) = (series("value"), series("rap"), series("num_limiteds"));
    let points = times
        .iter()
        .enumerate()
        .filter(|(_, time)| **time > 0)
        .map(|(index, time)| PlayerPoint {
            time: *time,
            value: value.get(index).copied().unwrap_or(0),
            rap: rap.get(index).copied().unwrap_or(0),
            collectibles: count.get(index).copied().unwrap_or(0),
        })
        .collect::<Vec<_>>();
    let rank = embedded(html, "player_details_data")
        .and_then(|details| details.get("rank").and_then(number))
        .filter(|rank| *rank > 0);
    let mut points = points;
    points.sort_by_key(|point| point.time);
    Ok(PlayerStats { rank, points })
}

/// A player's Heximons stats; `Ok(None)` when Heximons does not track them.
pub async fn fetch_player(user_id: u64) -> Result<Option<PlayerStats>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(format!("https://heximons.lol/player/{user_id}"))
        .send()
        .await
        .map_err(|error| format!("could not reach heximons.lol: {error}"))?;
    if response.status().as_u16() == 404 {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!("heximons.lol answered HTTP {}", response.status().as_u16()));
    }
    let html = response.text().await.map_err(|error| error.to_string())?;
    parse_player(&html).map(Some)
}

pub fn player_url(user_id: u64) -> String {
    format!("https://heximons.lol/player/{user_id}")
}

pub fn item_url(asset_id: u64) -> String {
    format!("https://heximons.lol/item/{asset_id}")
}

#[cfg(test)]
mod more_tests {
    use super::*;

    #[test]
    fn deals_sort_by_discount() {
        let mut values = Values::default();
        let item = |name: &str, value: u64, staff: bool, best: Option<u64>| ItemValue {
            name: name.to_string(),
            value,
            staff_value: staff.then_some(value),
            best_price: best,
            ..Default::default()
        };
        values.items.insert(1, item("Half", 1000, true, Some(500)));
        values.items.insert(2, item("Tenth off", 1000, false, Some(900)));
        values.items.insert(3, item("Pricier", 1000, true, Some(1200)));
        values.items.insert(4, item("Nobody selling", 1000, true, None));
        values.items.insert(5, item("Quarter", 400, false, Some(300)));
        let all = deals(&values, 5.0, false);
        let names: Vec<&str> = all.iter().map(|deal| deal.name.as_str()).collect();
        assert_eq!(names, vec!["Half", "Quarter", "Tenth off"]);
        assert!((all[0].discount - 50.0).abs() < 0.01);
        assert_eq!(deals(&values, 20.0, false).len(), 2);
        assert_eq!(deals(&values, 5.0, true).len(), 1);
    }

    /// The player page's shape (values made up).
    #[test]
    fn parses_player_history() {
        let html = r#"<script>var player_details_data = {"player_id":14028,"player_name":"zest","rank":419};
            var chart_data = {"num_points":3,"nominal_scan_time":[1794210574,1794210803,1794211257],
            "value":[13000,13200,13390],"rap":[10900,10950,10956],"num_limiteds":[15,15,16]};
            var old_chart_data = {"num_points":0};</script>"#;
        let stats = parse_player(html).unwrap();
        assert_eq!(stats.rank, Some(419));
        assert_eq!(stats.points.len(), 3);
        let latest = stats.latest().unwrap();
        assert_eq!((latest.value, latest.rap, latest.collectibles), (13390, 10956, 16));
        assert!(parse_player("<html></html>").is_err());
    }
}

// ---------------------------------------------------------------------------
// Heximons pages beyond the catalog: value changes, market activity, trade ads
// and the leaderboard. Each embeds its data as `var <name> = <json>;`.
// ---------------------------------------------------------------------------

/// The first complete JSON value after `var <name> =` (array or object). The
/// JSON parser stops at its end, so text inside strings cannot confuse it.
fn embedded_value(html: &str, name: &str) -> Option<Value> {
    let at = html.find(&format!("var {name}"))?;
    let rest = &html[at..];
    let start = rest.find(|character| character == '[' || character == '{')?;
    serde_json::Deserializer::from_str(&rest[start..]).into_iter::<Value>().next()?.ok()
}

async fn page(path: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(format!("https://heximons.lol{path}"))
        .send()
        .await
        .map_err(|error| format!("could not reach heximons.lol: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("heximons.lol answered HTTP {}", response.status().as_u16()));
    }
    response.text().await.map_err(|error| error.to_string())
}

fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// One staff change: `[time, assetId, 1, kind, old, new]`.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueChange {
    pub time: u64,
    pub asset_id: u64,
    /// 0 acronym, 1 value, 2 demand, 3 trend, 4 projected, 5 hyped, 6 rare.
    pub kind: u8,
    pub old: Option<String>,
    pub new: Option<String>,
}

impl ValueChange {
    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            0 => "Acronym",
            1 => "Value",
            2 => "Demand",
            3 => "Trend",
            4 => "Projected flag",
            5 => "Hyped flag",
            6 => "Rare flag",
            _ => "Change",
        }
    }

    /// The old or new side in words ("1,250 R$", "High", "Stable", "yes").
    pub fn describe(&self, side: &Option<String>) -> String {
        let Some(text) = side else { return "none".to_string() };
        let number: Option<u64> = text.parse().ok();
        match (self.kind, number) {
            (1, Some(value)) => format!("{} R$", group(value)),
            (2, Some(demand)) => ItemValue { demand: Some(demand as u8), ..Default::default() }
                .demand_label()
                .unwrap_or("?")
                .to_string(),
            (3, Some(trend)) => ItemValue { trend: Some(trend as u8), ..Default::default() }
                .trend_label()
                .unwrap_or("?")
                .to_string(),
            (4..=6, _) => {
                if text == "true" {
                    "yes".to_string()
                } else {
                    "no".to_string()
                }
            }
            _ => text.clone(),
        }
    }
}

fn group(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(character);
    }
    out
}

pub fn parse_value_changes(html: &str) -> Result<Vec<ValueChange>, String> {
    let list = embedded_value(html, "value_changes").ok_or("no value changes on the page")?;
    let mut changes: Vec<ValueChange> = list
        .as_array()
        .ok_or("value changes are not a list")?
        .iter()
        .filter_map(|entry| {
            let fields = entry.as_array()?;
            Some(ValueChange {
                time: number(fields.first()?)?,
                asset_id: number(fields.get(1)?)?,
                kind: number(fields.get(3)?)? as u8,
                old: fields.get(4).and_then(text_of),
                new: fields.get(5).and_then(text_of),
            })
        })
        .collect();
    changes.sort_by(|a, b| b.time.cmp(&a.time));
    Ok(changes)
}

pub async fn fetch_value_changes() -> Result<Vec<ValueChange>, String> {
    parse_value_changes(&page("/valuechanges").await?)
}

/// One detected sale: RAP before and after. The sale price follows from the
/// RAP rule (each sale moves RAP a tenth of the way to its price).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarketSale {
    pub time: u64,
    pub asset_id: u64,
    pub old_rap: u64,
    pub new_rap: u64,
}

impl MarketSale {
    pub fn price(&self) -> u64 {
        let moved = self.new_rap as i64 - self.old_rap as i64;
        (self.old_rap as i64 + moved * 10).max(0) as u64
    }
}

pub fn parse_market(html: &str) -> Result<Vec<MarketSale>, String> {
    let list = embedded_value(html, "initial_activity").ok_or("no market activity on the page")?;
    let mut sales: Vec<MarketSale> = list
        .as_array()
        .ok_or("market activity is not a list")?
        .iter()
        .filter_map(|entry| {
            let fields = entry.as_array()?;
            Some(MarketSale {
                time: number(fields.first()?)?,
                asset_id: number(fields.get(1)?)?,
                old_rap: number(fields.get(3)?)?,
                new_rap: number(fields.get(4)?)?,
            })
        })
        .collect();
    sales.sort_by(|a, b| b.time.cmp(&a.time));
    Ok(sales)
}

pub async fn fetch_market() -> Result<Vec<MarketSale>, String> {
    parse_market(&page("/marketactivity").await?)
}

/// A trade ad: what someone offers and which items they want.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TradeAd {
    pub id: u64,
    pub time: u64,
    pub player_id: u64,
    pub player_name: String,
    pub offer: Vec<u64>,
    pub offer_robux: u64,
    /// Requested asset ids (ads asking only for "any"/"upgrade" have none).
    pub request: Vec<u64>,
}

pub fn parse_trade_ads(html: &str) -> Result<Vec<TradeAd>, String> {
    let list = embedded_value(html, "trade_ads").ok_or("no trade ads on the page")?;
    let ids = |value: Option<&Value>| -> Vec<u64> {
        value
            .and_then(|side| side.get("items"))
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(number).collect())
            .unwrap_or_default()
    };
    let mut ads: Vec<TradeAd> = list
        .as_array()
        .ok_or("trade ads are not a list")?
        .iter()
        .filter_map(|entry| {
            let fields = entry.as_array()?;
            Some(TradeAd {
                id: number(fields.first()?)?,
                time: number(fields.get(1)?)?,
                player_id: number(fields.get(2)?)?,
                player_name: fields.get(3).and_then(Value::as_str).unwrap_or_default().to_string(),
                offer: ids(fields.get(4)),
                offer_robux: fields.get(4).and_then(|side| side.get("robux")).and_then(number).unwrap_or(0),
                request: ids(fields.get(5)),
            })
        })
        .collect();
    ads.sort_by(|a, b| b.time.cmp(&a.time));
    Ok(ads)
}

pub async fn fetch_trade_ads() -> Result<Vec<TradeAd>, String> {
    parse_trade_ads(&page("/trades").await?)
}

/// One leaderboard row.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ranked {
    pub rank: u64,
    pub player_id: u64,
    pub name: String,
    pub value: u64,
    pub rap: u64,
}

/// The leaderboard has no data list: each row is a link to the player whose
/// text reads "<name> Rank #N Value R$ X RAP R$ Y".
pub fn parse_leaderboard(html: &str) -> Vec<Ranked> {
    const LINK: &str = "href=\"/player/";
    let mut rows = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find(LINK) {
        rest = &rest[at + LINK.len()..];
        let Some(id_end) = rest.find('"') else { break };
        let Ok(player_id) = rest[..id_end].parse::<u64>() else { continue };
        let Some(close) = rest.find("</a>") else { break };
        // The visible part starts after the `>` that ends the link's own tag.
        let Some(open_end) = rest[..close].find('>') else { continue };
        let inner = &rest[open_end + 1..close];
        // Strip tags down to the visible words.
        let mut text = String::new();
        let mut in_tag = false;
        for character in inner.chars() {
            match character {
                '<' => in_tag = true,
                '>' => {
                    in_tag = false;
                    text.push(' ');
                }
                _ if !in_tag => text.push(character),
                _ => {}
            }
        }
        let words: Vec<&str> = text.split_whitespace().collect();
        let after = |label: &str| -> Option<u64> {
            let at = words.iter().position(|word| *word == label)?;
            words[at + 1..]
                .iter()
                .find_map(|word| word.trim_start_matches('#').replace(',', "").parse().ok())
        };
        if let (Some(rank), Some(value)) = (after("Rank"), after("Value")) {
            let name_end = words.iter().position(|word| *word == "Rank").unwrap_or(0);
            rows.push(Ranked {
                rank,
                player_id,
                name: html_unescape(&words[..name_end].join(" ")),
                value,
                rap: after("RAP").unwrap_or(0),
            });
        }
        rest = &rest[close..];
    }
    rows.sort_by_key(|row| row.rank);
    rows.dedup_by_key(|row| row.rank);
    rows
}

fn html_unescape(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

/// Both leaderboard pages (100 ranked players).
pub async fn fetch_leaderboard() -> Result<Vec<Ranked>, String> {
    let mut rows = parse_leaderboard(&page("/leaderboard").await?);
    if let Ok(second) = page("/leaderboard/2").await {
        rows.extend(parse_leaderboard(&second));
        rows.sort_by_key(|row| row.rank);
        rows.dedup_by_key(|row| row.rank);
    }
    if rows.is_empty() {
        return Err("the leaderboard page had no players".to_string());
    }
    Ok(rows)
}

#[cfg(test)]
mod page_tests {
    use super::*;

    #[test]
    fn value_changes_and_words() {
        let html = r#"<script>var value_changes = [[1794700000,907,1,1,"1250",null],[1794700100,907,1,2,"1","2"],[1794700200,5,1,0,null,"W];CJ"]];</script>"#;
        let changes = parse_value_changes(html).unwrap();
        assert_eq!(changes.len(), 3);
        assert_eq!(changes[0].kind_label(), "Acronym");
        assert_eq!(changes[0].new.as_deref(), Some("W];CJ"));
        let demand = &changes[1];
        assert_eq!((demand.describe(&demand.old), demand.describe(&demand.new)), ("Low".to_string(), "Normal".to_string()));
        let value = &changes[2];
        assert_eq!((value.describe(&value.old), value.describe(&value.new)), ("1,250 R$".to_string(), "none".to_string()));
    }

    #[test]
    fn market_sale_price_from_rap() {
        let html = "<script>var initial_activity = [[1794700000,5359,1,29,30,109591],[1794700100,66995,1,2064,2064,109592]];</script>";
        let sales = parse_market(html).unwrap();
        assert_eq!(sales[1].price(), 39);
        assert_eq!(sales[0].price(), 2064);
    }

    #[test]
    fn trade_ads_parse() {
        let html = r#"<script>var trade_ads = [[51,1794700000,77,"Someone",{"items":[3086],"robux":50},{"items":[2219,33519],"tags":[1]},null]];</script>"#;
        let ads = parse_trade_ads(html).unwrap();
        assert_eq!(ads[0].offer, vec![3086]);
        assert_eq!(ads[0].offer_robux, 50);
        assert_eq!(ads[0].request, vec![2219, 33519]);
        assert_eq!(ads[0].player_name, "Someone");
    }

    #[test]
    fn leaderboard_rows() {
        let html = r#"<a href="/player/12"><div><span>Big &amp; Rich</span></div><div>Rank</div><div>#1</div><div>Value</div><div>R$ 350,731</div><div>RAP</div><div>R$ 276,588</div></a>
            <a href="/player/34"><span>Second</span> Rank #2 Value R$ 333,316 RAP R$ 259,008</a><a href="/player/99">nav link</a>"#;
        let rows = parse_leaderboard(html);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (rows[0].rank, rows[0].player_id, rows[0].name.as_str(), rows[0].value, rows[0].rap),
            (1, 12, "Big & Rich", 350_731, 276_588)
        );
        assert_eq!(rows[1].name, "Second");
    }
}
