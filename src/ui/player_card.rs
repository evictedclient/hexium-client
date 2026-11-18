//! The hover card on player links: the player's headshot plus their Heximons
//! value, RAP and rank. Cards are fetched on first hover and kept for ten
//! minutes in a cache shared by every screen, so drawing one never needs `App`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use eframe::egui::{self, Response, RichText, Ui, WidgetText};

use crate::ui::{self, muted, Ctx, GOLD};
use crate::values::PlayerPoint;

const FRESH_FOR: Duration = Duration::from_secs(600);

#[derive(Clone)]
enum Card {
    Busy,
    Ready {
        at: Instant,
        avatar: Option<Arc<[u8]>>,
        /// `Ok(None)` when Heximons does not track the player.
        stats: Result<Option<(PlayerPoint, Option<u64>)>, String>,
    },
}

fn cards() -> &'static Mutex<HashMap<u64, Card>> {
    static CARDS: OnceLock<Mutex<HashMap<u64, Card>>> = OnceLock::new();
    CARDS.get_or_init(Default::default)
}

/// A link to a player that shows their card on hover. Returns the link's response.
pub fn link(ui: &mut Ui, cx: &Ctx, user_id: u64, text: impl Into<WidgetText>) -> Response {
    let response = ui.link(text);
    attach(response, cx, user_id)
}

/// Adds the player's hover card to any response (a link, a tile, a name chip).
pub fn attach(response: Response, cx: &Ctx, user_id: u64) -> Response {
    if user_id == 0 {
        return response;
    }
    response.on_hover_ui(|ui| show(ui, cx, user_id))
}

/// The card itself, starting the fetch the first time it is drawn.
pub fn show(ui: &mut Ui, cx: &Ctx, user_id: u64) {
    let state = {
        let mut cards = cards().lock().unwrap();
        let stale = match cards.get(&user_id) {
            None => true,
            Some(Card::Busy) => false,
            Some(Card::Ready { at, .. }) => at.elapsed() > FRESH_FOR,
        };
        if stale {
            // Keep showing the old card while the new one loads.
            if !cards.contains_key(&user_id) {
                cards.insert(user_id, Card::Busy);
            }
            fetch(cx, user_id);
            if let Some(Card::Ready { at, .. }) = cards.get_mut(&user_id) {
                *at = Instant::now();
            }
        }
        cards.get(&user_id).cloned().unwrap_or(Card::Busy)
    };

    ui.set_min_width(250.0);
    ui.horizontal(|ui| {
        let size = egui::vec2(64.0, 64.0);
        match &state {
            Card::Ready { avatar: Some(bytes), .. } => {
                ui.add(
                    egui::Image::from_bytes(format!("bytes://player-card/{user_id}"), bytes.clone())
                        .fit_to_exact_size(size)
                        .rounding(egui::Rounding::same(8.0)),
                );
            }
            _ => {
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                ui.painter().rect_filled(rect, egui::Rounding::same(8.0), ui::raised());
            }
        }
        ui.vertical(|ui| match &state {
            Card::Busy => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0));
                    muted(ui, "Loading…");
                });
            }
            Card::Ready { stats: Ok(Some((latest, rank))), .. } => {
                line(ui, "Value", latest.value);
                line(ui, "RAP", latest.rap);
                let mut extra = format!("{} collectibles", ui::money(latest.collectibles as i64));
                if let Some(rank) = rank {
                    extra = format!("Rank #{} · {extra}", ui::money(*rank as i64));
                }
                muted(ui, &extra);
            }
            Card::Ready { stats: Ok(None), .. } => muted(ui, "Heximons does not track this player."),
            Card::Ready { stats: Err(_), .. } => muted(ui, "Could not reach Heximons."),
        });
    });
    muted(ui, "Click to open their profile.");
}

fn line(ui: &mut Ui, label: &str, amount: u64) {
    ui.horizontal(|ui| {
        muted(ui, label);
        ui.label(RichText::new(format!("{} R$", ui::money(amount as i64))).strong().color(GOLD));
    });
}

fn fetch(cx: &Ctx, user_id: u64) {
    let api = cx.api.clone();
    let repaint = cx.egui.clone();
    cx.rt.spawn(async move {
        let avatar = async {
            let url = crate::api::endpoints::headshots(&api, &[user_id]).await.ok()?.remove(&user_id)?;
            api.fetch_image(&url).await.ok().map(Arc::<[u8]>::from)
        };
        let (avatar, stats) = tokio::join!(avatar, crate::values::fetch_player(user_id));
        let stats = stats.map(|stats| stats.and_then(|stats| stats.latest().map(|latest| (latest, stats.rank))));
        cards().lock().unwrap().insert(user_id, Card::Ready { at: Instant::now(), avatar, stats });
        repaint.request_repaint();
    });
}
