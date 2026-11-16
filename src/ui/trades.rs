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
            if ui.selectable_label(current == tab, label).clicked() && current != tab {
                picked = Some(tab);
            }
        }
    });
    if let Some(tab) = picked {
        app.trades.tab = tab.to_string();
        app.reload_trades();
    }
    ui.add_space(4.0);

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
                                        && ui.link(RichText::new(name).size(13.0).strong()).clicked()
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

    detail_window(app, cx, ui, &mut open_profile);

    if let Some(id) = open_profile {
        app.open_profile(id);
    }
}

fn detail_window(app: &mut App, cx: &Ctx, ui: &mut Ui, open_profile: &mut Option<u64>) {
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
    egui::Window::new(title)
        .id(egui::Id::new(("trade", id)))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(620.0)
        .show(ui.ctx(), |ui| match detail {
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
    let mut sides: Vec<&TradeOffer> = detail.offers.iter().collect();
    if let Some(me) = me {
        sides.sort_by_key(|offer| offer.user_id != me);
    }

    if sides.is_empty() {
        muted(ui, "The trade came back with no offers.");
        return;
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
                    if ui.link(RichText::new(&offer.user_name).size(12.0)).clicked() {
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
                        ui.label(RichText::new(&asset.name).size(12.0).strong());
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("RAP {}", ui::money(asset.rap as i64)))
                                    .size(11.0)
                                    .color(GOLD),
                            );
                            if let Some(serial) = asset.serial {
                                ui::pill(ui, &format!("#{serial}"), GOLD);
                            }
                        });
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
            if let Some(robux) = offer.robux {
                column.label(
                    RichText::new(format!("+ {} R$", ui::money(robux as i64)))
                        .size(12.0)
                        .color(GOLD),
                );
            }
        }
    });
    // Inbound offers that are still open can be answered here. Both buttons go
    // through the confirmation dialog, which lists both sides again.
    let inbound = app.trades_tab() == "inbound";
    if inbound && detail.is_active {
        if let Some(me) = me {
            let give = detail.offers.iter().find(|offer| offer.user_id == me);
            let get = detail.offers.iter().find(|offer| offer.user_id != me);
            if let (Some(give), Some(get)) = (give, get) {
                ui.add_space(10.0);
                ui.separator();
                ui.horizontal(|ui| {
                    let partner = get.user_name.clone();
                    if ui::primary_button(ui, "Accept…").clicked() {
                        app.ask(WriteAction::AnswerTrade {
                            trade_id: detail.id,
                            partner: partner.clone(),
                            accept: true,
                            give: TradeSide::from(give),
                            get: TradeSide::from(get),
                        });
                    }
                    if ui.button("Decline…").clicked() {
                        app.ask(WriteAction::AnswerTrade {
                            trade_id: detail.id,
                            partner,
                            accept: false,
                            give: TradeSide::from(give),
                            get: TradeSide::from(get),
                        });
                    }
                    if ui.button("Counter…").clicked() {
                        let partner_id = get.user_id;
                        let partner = get.user_name.clone();
                        let give_ids = give.assets.iter().map(|asset| asset.user_asset_id).collect();
                        let get_ids = get.assets.iter().map(|asset| asset.user_asset_id).collect();
                        app.open_trade_builder(partner_id, &partner, Some(detail.id), give_ids, get_ids);
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
