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
