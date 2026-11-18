//! The Market screen, from Heximons: staff value changes, detected sales, trade
//! ads and the leaderboard. Items open in the Catalog's details panel.

use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{self, RichText, Ui};

use crate::app::{App, Load};
use crate::ui::{self, accent, failed, h1, muted, waiting, Ctx, GOLD, GREEN, MUTED};

/// "5m ago", "3h ago", "2d ago".
pub fn ago(time: u64) -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_secs()).unwrap_or(0);
    let seconds = now.saturating_sub(time);
    match seconds {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", seconds / 60),
        3600..=86_399 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    h1(ui, "Market");
    muted(ui, "From Heximons: staff value changes, sales as they happen, trade ads and the richest players.");
    ui.add_space(8.0);

    let mut refresh = false;
    ui.horizontal(|ui| {
        for (index, label) in ["Value changes", "Activity", "Trade ads", "Leaderboard"].into_iter().enumerate() {
            if ui.selectable_label(app.market.tab == index as u8, label).clicked() {
                app.market.tab = index as u8;
            }
        }
        ui.add_space(12.0);
        match app.market.tab {
            0 | 1 => {
                ui.checkbox(&mut app.market.mine_only, "Mine & watchlist only");
            }
            2 => {
                ui.checkbox(&mut app.market.ads_for_me, "Wanting my items");
            }
            _ => {}
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            refresh = ui.button("Refresh").clicked();
        });
    });
    if refresh {
        match app.market.tab {
            0 => app.market.changes = Load::Idle,
            1 => app.market.sales = Load::Idle,
            2 => app.market.ads = Load::Idle,
            _ => app.market.leaderboard = Load::Idle,
        }
    }
    app.load_market();
    ui.add_space(6.0);

    match app.market.tab {
        0 => changes(app, cx, ui),
        1 => activity(app, cx, ui),
        2 => ads(app, cx, ui),
        _ => leaderboard(app, cx, ui),
    }
}

fn item_name(app: &App, asset: u64) -> String {
    app.values.get(asset).map(|item| item.name.clone()).unwrap_or_else(|| format!("Item {asset}"))
}

fn changes(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    let list = match app.market.changes.clone() {
        Load::Ready(list) => list,
        Load::Err(error) => return failed(ui, &format!("Could not load value changes: {error}")),
        _ => return waiting(ui, "value changes"),
    };
    let mine = app.my_assets();
    let shown: Vec<_> = list
        .iter()
        .filter(|change| !app.market.mine_only || mine.contains(&change.asset_id))
        .take(150)
        .collect();
    if shown.is_empty() {
        muted(ui, "No changes to show.");
        return;
    }
    let mut open: Option<(u64, String)> = None;
    let width = (ui.available_width() - 4.0).max(320.0);
    for change in shown {
        let name = item_name(app, change.asset_id);
        let thumb = app.values.get(change.asset_id).and_then(|item| item.thumb.clone());
        let card = ui::tile_button(ui, width, false, |ui| {
            ui.horizontal(|ui| {
                app.thumbs.show(ui, cx, thumb.as_deref(), egui::vec2(36.0, 36.0));
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&name).font(ui::semibold(12.5)));
                        if mine.contains(&change.asset_id) {
                            ui::pill(ui, "YOURS", accent());
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(change.kind_label()).size(11.5).color(MUTED));
                        ui.label(RichText::new(change.describe(&change.old)).size(12.0));
                        ui.label(RichText::new("→").size(12.0).color(MUTED));
                        let rising = matches!(
                            (change.old.as_deref().and_then(|text| text.parse::<u64>().ok()), change.new.as_deref().and_then(|text| text.parse::<u64>().ok())),
                            (Some(old), Some(new)) if new > old
                        );
                        ui.label(
                            RichText::new(change.describe(&change.new))
                                .font(ui::semibold(12.0))
                                .color(if rising { GREEN } else { GOLD }),
                        );
                        ui.label(RichText::new(format!("· {}", ago(change.time))).size(11.0).color(MUTED));
                    });
                });
            });
        });
        if card.clicked() {
            open = Some((change.asset_id, name));
        }
    }
    if let Some((asset, name)) = open {
        app.open_item(asset, &name);
    }
}

fn activity(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    let list = match app.market.sales.clone() {
        Load::Ready(list) => list,
        Load::Err(error) => return failed(ui, &format!("Could not load market activity: {error}")),
        _ => return waiting(ui, "market activity"),
    };
    let mine = app.my_assets();
    let shown: Vec<_> = list
        .iter()
        .filter(|sale| !app.market.mine_only || mine.contains(&sale.asset_id))
        .collect();
    if shown.is_empty() {
        muted(ui, "No sales to show.");
        return;
    }
    muted(ui, "Sale prices are worked out from how far each sale moved the RAP, so they are approximate.");
    let mut open: Option<(u64, String)> = None;
    let width = (ui.available_width() - 4.0).max(320.0);
    for sale in shown {
        let name = item_name(app, sale.asset_id);
        let thumb = app.values.get(sale.asset_id).and_then(|item| item.thumb.clone());
        let card = ui::tile_button(ui, width, false, |ui| {
            ui.horizontal(|ui| {
                app.thumbs.show(ui, cx, thumb.as_deref(), egui::vec2(36.0, 36.0));
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&name).font(ui::semibold(12.5)));
                        if mine.contains(&sale.asset_id) {
                            ui::pill(ui, "YOURS", accent());
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("sold ≈ {} R$", ui::money(sale.price() as i64)))
                                .font(ui::semibold(12.0))
                                .color(GOLD),
                        );
                        ui.label(
                            RichText::new(format!(
                                "RAP {} → {}",
                                ui::money(sale.old_rap as i64),
                                ui::money(sale.new_rap as i64)
                            ))
                            .size(11.5)
                            .color(if sale.new_rap >= sale.old_rap { GREEN } else { ui::DANGER }),
                        );
                        ui.label(RichText::new(format!("· {}", ago(sale.time))).size(11.0).color(MUTED));
                    });
                });
            });
        });
        if card.clicked() {
            open = Some((sale.asset_id, name));
        }
    }
    if let Some((asset, name)) = open {
        app.open_item(asset, &name);
    }
}

fn ads(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    let list = match app.market.ads.clone() {
        Load::Ready(list) => list,
        Load::Err(error) => return failed(ui, &format!("Could not load trade ads: {error}")),
        _ => return waiting(ui, "trade ads"),
    };
    let mine = app.my_assets();
    let owned: std::collections::HashSet<u64> = app
        .inventory
        .data
        .ready()
        .map(|bundle| bundle.collectibles.iter().map(|item| item.id).collect())
        .unwrap_or_default();
    let me = app.session.ready().map(|session| session.user.id).unwrap_or(0);
    let shown: Vec<_> = list
        .iter()
        .filter(|ad| ad.player_id != me)
        .filter(|ad| !app.market.ads_for_me || ad.request.iter().any(|asset| owned.contains(asset)))
        .take(120)
        .collect();
    if shown.is_empty() {
        muted(ui, if app.market.ads_for_me { "No ads asking for items you own right now." } else { "No trade ads." });
        return;
    }
    let mut trade: Option<crate::values::TradeAd> = None;
    let mut open_item: Option<(u64, String)> = None;
    let mut profile: Option<u64> = None;
    let width = (ui.available_width() - 4.0).max(320.0);
    for ad in shown {
        ui::tile(ui, width, |ui| {
            ui.horizontal(|ui| {
                if ui::player_card::link(ui, cx, ad.player_id, RichText::new(&ad.player_name).font(ui::semibold(13.0))).clicked() {
                    profile = Some(ad.player_id);
                }
                muted(ui, &ago(ad.time));
                let wanted = ad.request.iter().filter(|asset| owned.contains(asset)).count();
                if wanted > 0 {
                    ui::pill(ui, &format!("WANTS {wanted} OF YOURS"), GREEN);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Trade with them…").clicked() {
                        trade = Some(ad.clone());
                    }
                });
            });
            let mut side = |ui: &mut Ui, label: &str, assets: &[u64], robux: u64| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(label).size(11.5).color(MUTED));
                    if assets.is_empty() && robux == 0 {
                        ui.label(RichText::new("anything (tags only)").size(11.5).color(MUTED));
                    }
                    let mut total = 0;
                    for asset in assets {
                        let item = app.values.get(*asset);
                        total += item.map(|item| item.value).unwrap_or(0);
                        let name = item.map(|item| item.name.clone()).unwrap_or_else(|| format!("Item {asset}"));
                        let color = if mine.contains(asset) { accent() } else { ui::INK };
                        if ui.link(RichText::new(&name).size(12.0).color(color)).clicked() {
                            open_item = Some((*asset, name.clone()));
                        }
                    }
                    if robux > 0 {
                        ui.label(RichText::new(format!("+ {} R$", ui::money(robux as i64))).size(12.0).color(GOLD));
                    }
                    if total > 0 {
                        ui.label(RichText::new(format!("· value {}", ui::money(total as i64))).size(11.5).color(GOLD));
                    }
                });
            };
            side(ui, "Offers", &ad.offer, ad.offer_robux);
            side(ui, "Wants", &ad.request, 0);
        });
    }
    if let Some(id) = profile {
        app.open_profile(id);
    }
    if let Some(ad) = trade {
        app.trade_from_ad(&ad);
    }
    if let Some((asset, name)) = open_item {
        app.open_item(asset, &name);
    }
}

fn leaderboard(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    let list = match app.market.leaderboard.clone() {
        Load::Ready(list) => list,
        Load::Err(error) => return failed(ui, &format!("Could not load the leaderboard: {error}")),
        _ => return waiting(ui, "leaderboard"),
    };
    let me = app.session.ready().map(|session| session.user.id).unwrap_or(0);
    if let Some(Load::Ready(Some(stats))) = app.player_stats.get(&me).cloned() {
        if let (Some(rank), Some(latest)) = (stats.rank, stats.latest()) {
            ui.label(
                RichText::new(format!("You: #{rank} · value {}", ui::money(latest.value as i64)))
                    .font(ui::semibold(13.0))
                    .color(accent()),
            );
            ui.add_space(4.0);
        }
    } else if me != 0 {
        app.player_stats(me, false);
    }
    let mut profile: Option<u64> = None;
    egui::Grid::new("leaderboard").striped(true).spacing(egui::vec2(18.0, 6.0)).show(ui, |ui| {
        for heading in ["#", "Player", "Value", "RAP"] {
            ui.label(RichText::new(heading).size(11.5).color(MUTED));
        }
        ui.end_row();
        for row in &list {
            let you = row.player_id == me;
            let color = if you { accent() } else { ui::INK };
            ui.label(RichText::new(format!("{}", row.rank)).color(color));
            if ui::player_card::link(ui, cx, row.player_id, RichText::new(&row.name).color(color)).clicked() {
                profile = Some(row.player_id);
            }
            ui.label(RichText::new(ui::money(row.value as i64)).color(GOLD));
            ui.label(RichText::new(ui::money(row.rap as i64)).color(MUTED));
            ui.end_row();
        }
    });
    if let Some(id) = profile {
        app.open_profile(id);
    }
}
