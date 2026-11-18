use eframe::egui::{self, Color32, RichText, Ui};

use crate::api::endpoints::TRADE_TABS;
use crate::api::models::{TradeDetail, TradeOffer};
use crate::app::{App, Load, TradeSide, WriteAction};
use crate::ui::{self, failed, h1, muted, waiting, Ctx, accent, DANGER, GOLD, GREEN, MUTED};

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    app.load_trades();

    h1(ui, "Trades");
    muted(ui, "Offers you have sent and received. Open an inbound offer to accept or decline it.");
    ui.add_space(8.0);

    let current = app.trades_tab();
    let mut picked: Option<&str> = None;
    ui.horizontal(|ui| {
        for (tab, label) in TRADE_TABS {
            let on = current == tab && !app.trades.show_stats;
            if ui.selectable_label(on, label).clicked() && !on {
                picked = Some(tab);
            }
        }
        ui.add_space(10.0);
        if ui.selectable_label(app.trades.show_stats, "Stats").clicked() {
            app.trades.show_stats = !app.trades.show_stats;
        }
    });
    if let Some(tab) = picked {
        app.trades.show_stats = false;
        app.trades.tab = tab.to_string();
        app.reload_trades();
    }
    ui.add_space(4.0);
    if app.trades.show_stats {
        stats(app, cx, ui);
        return;
    }

    let mut open_trade: Option<u64> = None;
    let mut open_profile: Option<u64> = None;

    match app.trades.data.clone() {
        Load::Ready(page) => {
            if page.items.is_empty() {
                muted(ui, "No trades in this tab.");
            } else {
                muted(ui, &format!("{} trade(s) loaded", page.items.len()));
                ui.add_space(4.0);
                // Measured once, so the rows do not creep wider one by one.
                let width = (ui.available_width() - 16.0).max(320.0);
                for trade in &page.items {
                    let icon = page.icons.get(&trade.partner_id).cloned();
                    ui::tile(ui, width, |ui| {
                        ui.horizontal(|ui| {
                            app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(40.0, 40.0));
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    let name = if trade.partner_name.is_empty() {
                                        "unknown user"
                                    } else {
                                        trade.partner_name.as_str()
                                    };
                                    if trade.partner_id != 0
                                        && ui::player_card::link(ui, cx, trade.partner_id, RichText::new(name).size(13.0).strong()).clicked()
                                    {
                                        open_profile = Some(trade.partner_id);
                                    }
                                    ui::pill(ui, &trade.status.to_uppercase(), status_color(&trade.status));
                                });
                                let mut when = format!("created {}", stamp(&trade.created));
                                if trade.is_active && !trade.expiration.is_empty() {
                                    when.push_str(&format!(" · expires {}", stamp(&trade.expiration)));
                                }
                                muted(ui, &when);
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let shown = app.trades.selected == Some(trade.id);
                                if ui.small_button(if shown { "Shown" } else { "Details" }).clicked() {
                                    open_trade = Some(trade.id);
                                }
                            });
                        });
                    });
                }

                ui.add_space(6.0);
                if app.trades.loading_more {
                    waiting(ui, "more trades");
                } else if page.next.is_some() {
                    if ui.button("Load more").clicked() {
                        app.load_more_trades();
                    }
                } else {
                    muted(ui, "End of the list.");
                }
                if let Some(error) = &app.trades.more_error {
                    failed(ui, error);
                }
            }
        }
        Load::Err(error) => {
            failed(ui, &error);
            if ui.button("Retry").clicked() {
                app.reload_trades();
            }
        }
        _ => waiting(ui, "trades"),
    }

    if let Some(id) = open_trade {
        app.open_trade(id);
    }

    if let Some(id) = open_profile {
        app.open_profile(id);
    }
}

/// The selected trade, in the docked panel.
pub fn dock(app: &mut App, cx: &Ctx, ctx: &egui::Context) {
    let mut open_profile: Option<u64> = None;
    detail_window(app, cx, ctx, &mut open_profile);
    if let Some(id) = open_profile {
        app.open_profile(id);
    }
}

fn detail_window(app: &mut App, cx: &Ctx, ctx: &egui::Context, open_profile: &mut Option<u64>) {
    let Some(id) = app.trades.selected else {
        return;
    };
    let partner = app
        .trades
        .data
        .ready()
        .and_then(|page| page.items.iter().find(|trade| trade.id == id))
        .map(|trade| trade.partner_name.clone())
        .filter(|name| !name.is_empty());
    let title = match partner {
        Some(name) => format!("Trade with {name}"),
        None => format!("Trade {id}"),
    };
    let me = app.session.ready().map(|session| session.user.id);
    let detail = app.trades.detail.clone();

    let mut open = true;
    ui::dock(ctx, "trade", 560.0, egui::Id::new(("trade", id)), title, &mut open, |ui| match detail {
            Load::Ready(detail) => detail_body(app, cx, ui, &detail, me, open_profile),
            Load::Err(error) => {
                failed(ui, &error);
                if ui.button("Retry").clicked() {
                    app.open_trade(id);
                }
            }
            _ => waiting(ui, "trade details"),
        });
    if !open {
        app.trades.selected = None;
        app.trades.detail = Load::Idle;
    }
}

fn detail_body(
    app: &mut App,
    cx: &Ctx,
    ui: &mut Ui,
    detail: &TradeDetail,
    me: Option<u64>,
    open_profile: &mut Option<u64>,
) {
    ui.horizontal(|ui| {
        ui::pill(ui, &detail.status.to_uppercase(), status_color(&detail.status));
        let mut when = format!("created {}", stamp(&detail.created));
        // Every trade carries an expiration; it only means something while open.
        if detail.is_active && !detail.expiration.is_empty() {
            when.push_str(&format!(" · expires {}", stamp(&detail.expiration)));
        }
        muted(ui, &when);
    });
    ui.add_space(6.0);

    // Your side first. The API's own order is not reliable, so match on user id.
    let mut open_item: Option<(u64, String)> = None;
    let mut sides: Vec<&TradeOffer> = detail.offers.iter().collect();
    if let Some(me) = me {
        sides.sort_by_key(|offer| offer.user_id != me);
    }

    if sides.is_empty() {
        muted(ui, "The trade came back with no offers.");
        return;
    }

    // Price history for each item's small chart (fetched once, then cached).
    let mut histories = std::collections::HashMap::new();
    for asset in sides.iter().flat_map(|offer| offer.assets.iter()) {
        if !histories.contains_key(&asset.asset_id) {
            histories.insert(asset.asset_id, app.history_of(asset.asset_id));
        }
    }

    ui.columns(sides.len().min(2), |columns| {
        for (column, offer) in columns.iter_mut().zip(sides.iter()) {
            let mine = me == Some(offer.user_id);
            let heading = if mine {
                "You give".to_string()
            } else if me.is_some() {
                "You get".to_string()
            } else {
                offer.user_name.clone()
            };
            column.label(RichText::new(heading).size(14.0).strong());
            column.horizontal(|ui| {
                muted(ui, if mine { "your items" } else { "from" });
                if !mine && offer.user_id != 0 {
                    if ui::player_card::link(ui, cx, offer.user_id, RichText::new(&offer.user_name).size(12.0)).clicked() {
                        *open_profile = Some(offer.user_id);
                    }
                }
            });
            column.add_space(4.0);

            if offer.assets.is_empty() {
                muted(column, "No items.");
            }
            for asset in &offer.assets {
                let icon = detail.icons.get(&asset.asset_id).cloned();
                column.horizontal(|ui| {
                    app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(44.0, 44.0));
                    ui.vertical(|ui| {
                        if ui.link(RichText::new(&asset.name).size(12.0).strong()).clicked() {
                            open_item = Some((asset.asset_id, asset.name.clone()));
                        }
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("RAP {}", ui::money(asset.rap as i64)))
                                    .size(11.0)
                                    .color(GOLD),
                            );
                            if let Some(serial) = asset.serial {
                                ui::pill(ui, &format!("#{serial}"), GOLD);
                            }
                            if let Some(history) = histories.get(&asset.asset_id) {
                                ui::analyze::sparkline(ui, history);
                            }
                        });
                        ui::analyze::value_line(ui, &app.values, asset.asset_id);
                    });
                });
            }

            column.add_space(4.0);
            column.separator();
            column.label(
                RichText::new(format!("Total RAP {}", ui::money(offer.total_rap() as i64)))
                    .size(12.0)
                    .strong()
                    .color(GOLD),
            );
            if !app.values.items.is_empty() {
                let value = side_value(&app.values, offer);
                column.label(
                    RichText::new(format!("Total value {}", ui::money(value as i64)))
                        .size(12.0)
                        .color(GOLD),
                );
            }
            if let Some(robux) = offer.robux {
                column.label(
                    RichText::new(format!("+ {} R$", ui::money(robux as i64)))
                        .size(12.0)
                        .color(GOLD),
                );
            }
        }
    });
    if let Some((asset, name)) = open_item.take() {
        app.open_item(asset, &name);
    }

    // The analyzer's verdict, for you.
    if let Some(me) = me {
        let mine = detail.offers.iter().find(|offer| offer.user_id == me);
        let theirs = detail.offers.iter().find(|offer| offer.user_id != me);
        if let (Some(mine), Some(theirs)) = (mine, theirs) {
            ui.add_space(8.0);
            let side = |offer: &TradeOffer| ui::analyze::Side {
                rap: offer.total_rap(),
                value: side_value(&app.values, offer),
                robux: offer.robux.unwrap_or(0),
            };
            let weak = ui::analyze::weak_demand(
                &app.values,
                theirs.assets.iter().map(|asset| (asset.asset_id, asset.name.as_str())),
            );
            ui::analyze::verdict(ui, side(mine), side(theirs), &weak);
        }
    }

    // Inbound offers that are still open can be answered here. Both buttons go
    // through the confirmation dialog, which lists both sides again.
    let inbound = app.trades_tab() == "inbound";
    if inbound && detail.is_active {
        if let Some(me) = me {
            let give = detail.offers.iter().find(|offer| offer.user_id == me);
            let get = detail.offers.iter().find(|offer| offer.user_id != me);
            if let (Some(give), Some(get)) = (give, get) {
                let give_side = TradeSide { value: side_value(&app.values, give), ..TradeSide::from(give) };
                let get_side = TradeSide { value: side_value(&app.values, get), ..TradeSide::from(get) };
                ui.add_space(10.0);
                ui.separator();
                ui.horizontal(|ui| {
                    let partner = get.user_name.clone();
                    if ui::primary_button(ui, "Accept…").clicked() {
                        app.ask(WriteAction::AnswerTrade {
                            trade_id: detail.id,
                            partner: partner.clone(),
                            accept: true,
                            give: give_side.clone(),
                            get: get_side.clone(),
                        });
                    }
                    if ui.button("Decline…").clicked() {
                        app.ask(WriteAction::AnswerTrade {
                            trade_id: detail.id,
                            partner,
                            accept: false,
                            give: give_side.clone(),
                            get: get_side.clone(),
                        });
                    }
                    if ui.button("Counter…").clicked() {
                        let partner_id = get.user_id;
                        let partner = get.user_name.clone();
                        let give_ids = give.assets.iter().map(|asset| asset.user_asset_id).collect();
                        let get_ids = get.assets.iter().map(|asset| asset.user_asset_id).collect();
                        app.open_trade_builder(
                            partner_id,
                            &partner,
                            Some(detail.id),
                            give_ids,
                            get_ids,
                            (give.robux, get.robux),
                        );
                    }
                    muted(ui, "You confirm on the next step.");
                });
            }
        }
    }

}

fn status_color(status: &str) -> Color32 {
    match status.to_ascii_lowercase().as_str() {
        "open" | "pending" => accent(),
        "completed" => GREEN,
        "declined" | "rejected" | "expired" | "countered" => DANGER,
        _ => MUTED,
    }
}

/// `2026-11-15T18:50:33Z` → `2026-11-15 18:50`.
fn stamp(text: &str) -> String {
    text.replace('T', " ").chars().take(16).collect()
}

/// One side's worth by Heximons value (RAP for items it does not track).
pub fn side_value(values: &crate::values::Values, offer: &TradeOffer) -> u64 {
    offer
        .assets
        .iter()
        .map(|asset| values.value_or(asset.asset_id, asset.rap))
        .sum()
}

/// Your last completed trades, judged at today's Heximons values.
fn stats(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    app.load_trade_stats();
    let mut open_profile_name: Option<String> = None;
    let mut open_profile_id: Option<u64> = None;
    match app.trades.stats.clone() {
        Load::Ready(stats) => {
            if stats.rows.is_empty() {
                muted(ui, "No completed trades yet.");
                return;
            }
            let total_value: i64 = stats.rows.iter().map(|row| row.gain_value).sum();
            let total_rap: i64 = stats.rows.iter().map(|row| row.gain_rap).sum();
            let signed = |amount: i64| format!("{}{}", if amount > 0 { "+" } else { "" }, ui::money(amount));
            let color = |amount: i64| if amount > 0 { ui::GREEN } else if amount < 0 { ui::DANGER } else { MUTED };
            ui::wrap(ui, |ui| {
                ui::stat(ui, "TRADES", &stats.rows.len().to_string(), ui::accent());
                ui::stat(ui, "BY VALUE", &signed(total_value), color(total_value));
                ui::stat(ui, "BY RAP", &signed(total_rap), color(total_rap));
                let won = stats.rows.iter().filter(|row| row.gain_value > 0).count();
                ui::stat(ui, "WON", &format!("{won} of {}", stats.rows.len()), ui::accent());
            });
            muted(ui, "What you got minus what you gave, at today's values (RAP where Heximons has none), Robux included.");
            let best = stats.rows.iter().max_by_key(|row| row.gain_value);
            let worst = stats.rows.iter().min_by_key(|row| row.gain_value);
            if let (Some(best), Some(worst)) = (best, worst) {
                ui.label(RichText::new(format!("Best: with {} · {}", best.partner, signed(best.gain_value))).color(ui::GREEN));
                ui.label(RichText::new(format!("Worst: with {} · {}", worst.partner, signed(worst.gain_value))).color(ui::DANGER));
            }
            ui.add_space(6.0);
            ui::eyebrow(ui, "Who you trade with");
            ui.horizontal_wrapped(|ui| {
                for (name, count) in stats.partners.iter().take(10) {
                    if ui.link(format!("{name} ×{count}")).clicked() {
                        open_profile_name = Some(name.clone());
                    }
                }
            });
            ui.add_space(6.0);
            ui::eyebrow(ui, "Each trade");
            egui::Grid::new("trade-stats").striped(true).spacing(egui::vec2(16.0, 4.0)).show(ui, |ui| {
                for heading in ["When", "With", "By value", "By RAP"] {
                    ui.label(RichText::new(heading).size(11.5).color(MUTED));
                }
                ui.end_row();
                for row in &stats.rows {
                    ui.label(RichText::new(stamp(&row.created)).size(11.5).color(MUTED));
                    if row.partner_id != 0 && ui::player_card::link(ui, cx, row.partner_id, &row.partner).clicked() {
                        open_profile_id = Some(row.partner_id);
                    } else if row.partner_id == 0 {
                        ui.label(&row.partner);
                    }
                    ui.label(RichText::new(signed(row.gain_value)).color(color(row.gain_value)));
                    ui.label(RichText::new(signed(row.gain_rap)).size(11.5).color(color(row.gain_rap)));
                    ui.end_row();
                }
            });
            if ui.button("Recalculate").clicked() {
                app.trades.stats = Load::Idle;
            }
        }
        Load::Err(error) => failed(ui, &format!("Could not work out the stats: {error}")),
        _ => waiting(ui, "your completed trades (this reads each one)"),
    }
    if let Some(id) = open_profile_id {
        app.open_profile(id);
    }
    if let Some(name) = open_profile_name {
        // Partners are listed by name; their id is in the trade list rows.
        if let Some(id) = app
            .trades
            .data
            .ready()
            .and_then(|page| page.items.iter().find(|trade| trade.partner_name == name))
            .map(|trade| trade.partner_id)
        {
            app.open_profile(id);
        }
    }
}
