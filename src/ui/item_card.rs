//! The hover card on item links: thumbnail, RAP, value (yours when you set
//! one), demand, and how the price moved over the last week. Sales come from
//! Hexium's resale data, fetched on first hover and kept for ten minutes in a
//! shared cache; values come from a copy of the app's values, so drawing a card
//! never needs `App`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Response, RichText, Ui, WidgetText};

use crate::api::models::ResaleData;
use crate::app::Load;
use crate::ui::{self, muted, Ctx, GOLD, GREEN, MUTED};
use crate::values::Values;

const FRESH_FOR: Duration = Duration::from_secs(600);
const WEEK_DAYS: i64 = 7;

#[derive(Clone)]
enum Card {
    Busy,
    Ready {
        at: Instant,
        thumb: Option<Arc<[u8]>>,
        sales: Result<ResaleData, String>,
    },
}

fn cards() -> &'static Mutex<HashMap<u64, Card>> {
    static CARDS: OnceLock<Mutex<HashMap<u64, Card>>> = OnceLock::new();
    CARDS.get_or_init(Default::default)
}

fn values() -> &'static RwLock<Arc<Values>> {
    static VALUES: OnceLock<RwLock<Arc<Values>>> = OnceLock::new();
    VALUES.get_or_init(Default::default)
}

/// The values as last handed over (Heximons plus your overrides).
pub fn current_values() -> Arc<Values> {
    values().read().unwrap().clone()
}

/// Called whenever the app's values change (Heximons refresh or your overrides).
pub fn set_values(current: &Values) {
    *values().write().unwrap() = Arc::new(current.clone());
}

/// A link to an item that shows its card on hover.
pub fn link(ui: &mut Ui, cx: &Ctx, asset_id: u64, text: impl Into<WidgetText>) -> Response {
    let response = ui.link(text);
    attach(response, cx, asset_id)
}

/// Adds the item's hover card to any response.
pub fn attach(response: Response, cx: &Ctx, asset_id: u64) -> Response {
    if asset_id == 0 {
        return response;
    }
    response.on_hover_ui(|ui| show(ui, cx, asset_id))
}

pub fn show(ui: &mut Ui, cx: &Ctx, asset_id: u64) {
    let state = {
        let mut cards = cards().lock().unwrap();
        let stale = match cards.get(&asset_id) {
            None => true,
            Some(Card::Busy) => false,
            Some(Card::Ready { at, .. }) => at.elapsed() > FRESH_FOR,
        };
        if stale {
            if !cards.contains_key(&asset_id) {
                cards.insert(asset_id, Card::Busy);
            }
            let thumb_url = values().read().unwrap().get(asset_id).and_then(|item| item.thumb.clone());
            fetch(cx, asset_id, thumb_url);
            if let Some(Card::Ready { at, .. }) = cards.get_mut(&asset_id) {
                *at = Instant::now();
            }
        }
        cards.get(&asset_id).cloned().unwrap_or(Card::Busy)
    };
    let valued = values().read().unwrap().get(asset_id).cloned();

    ui.set_min_width(270.0);
    ui.horizontal_top(|ui| {
        let size = egui::vec2(72.0, 72.0);
        match &state {
            Card::Ready { thumb: Some(bytes), .. } => {
                ui.add(
                    egui::Image::from_bytes(format!("bytes://item-card/{asset_id}"), bytes.clone())
                        .fit_to_exact_size(size)
                        .rounding(egui::Rounding::same(8.0)),
                );
            }
            _ => {
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                ui.painter().rect_filled(rect, egui::Rounding::same(8.0), ui::raised());
            }
        }
        ui.vertical(|ui| {
            if let Some(item) = &valued {
                ui.label(RichText::new(&item.name).strong());
            }
            // RAP: Hexium's own figure once loaded, Heximons' until then.
            let rap = match &state {
                Card::Ready { sales: Ok(sales), .. } if sales.rap > 0 => Some(sales.rap),
                _ => valued.as_ref().map(|item| item.rap).filter(|rap| *rap > 0),
            };
            if let Some(rap) = rap {
                amount(ui, "RAP", rap, GOLD);
            }
            match &valued {
                Some(item) => {
                    let label = if item.overridden { "Your value" } else { "Value" };
                    amount(ui, label, item.value, GOLD);
                    let mut notes: Vec<String> = Vec::new();
                    if let Some(demand) = item.demand_label() {
                        notes.push(format!("{demand} demand"));
                    }
                    if let Some(trend) = item.trend_label() {
                        notes.push(trend.to_string());
                    }
                    if item.staff_value.is_none() && !item.overridden {
                        notes.push("no staff value".to_string());
                    }
                    if !notes.is_empty() {
                        muted(ui, &notes.join(" · "));
                    }
                }
                None => muted(ui, "Heximons has no value for this item."),
            }
        });
    });

    ui.add_space(4.0);
    match &state {
        Card::Busy => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(12.0));
                muted(ui, "Loading the week's sales…");
            });
        }
        Card::Ready { sales: Err(_), .. } => muted(ui, "Could not load sales."),
        Card::Ready { sales: Ok(sales), .. } => week(ui, sales),
    }
    muted(ui, "Click to open its details.");
}

/// The last seven days: a sparkline, the change in average price and the sales.
fn week(ui: &mut Ui, sales: &ResaleData) {
    let today = (SystemTime::now().duration_since(UNIX_EPOCH).map(|time| time.as_secs()).unwrap_or(0) / 86_400) as i64;
    let mut recent = sales.clone();
    recent.points.retain(|point| point.day > today - WEEK_DAYS && point.price > 0);
    let sold: u64 = recent.points.iter().map(|point| point.volume).sum();
    ui.horizontal(|ui| {
        muted(ui, "Last week");
        if recent.points.is_empty() {
            muted(ui, "· no sales");
            return;
        }
        ui::analyze::sparkline(ui, &Load::Ready(recent.clone()));
        let first = recent.points.first().map(|point| point.price).unwrap_or(0);
        let last = recent.points.last().map(|point| point.price).unwrap_or(0);
        if recent.points.len() > 1 && first > 0 {
            let change = (last as f64 - first as f64) / first as f64 * 100.0;
            let (sign, color) = match change {
                change if change > 0.5 => ("+", GREEN),
                change if change < -0.5 => ("", ui::DANGER),
                _ => ("", MUTED),
            };
            ui.label(RichText::new(format!("{sign}{change:.0}%")).strong().color(color));
        }
        muted(ui, &format!("· {sold} sale{}", if sold == 1 { "" } else { "s" }));
    });
}

fn amount(ui: &mut Ui, label: &str, value: u64, color: egui::Color32) {
    ui.horizontal(|ui| {
        muted(ui, label);
        ui.label(RichText::new(format!("{} R$", ui::money(value as i64))).strong().color(color));
    });
}

fn fetch(cx: &Ctx, asset_id: u64, thumb_url: Option<String>) {
    let api = cx.api.clone();
    let repaint = cx.egui.clone();
    cx.rt.spawn(async move {
        let thumb = async {
            let url = match thumb_url.filter(|url| url.starts_with("http")) {
                Some(url) => url,
                None => crate::api::endpoints::asset_icons(&api, &[asset_id]).await.ok()?.remove(&asset_id)?,
            };
            api.fetch_image(&url).await.ok().map(Arc::<[u8]>::from)
        };
        let sales = async { crate::api::endpoints::resale_data(&api, asset_id).await.map_err(|error| error.to_string()) };
        let (thumb, sales) = tokio::join!(thumb, sales);
        cards().lock().unwrap().insert(asset_id, Card::Ready { at: Instant::now(), thumb, sales });
        repaint.request_repaint();
    });
}
