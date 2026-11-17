use eframe::egui::{self, RichText, Ui};

use crate::app::{App, Load, WriteAction};
use crate::ui::{self, failed, h1, h2, muted, waiting, Ctx, accent, GOLD, GREEN, MUTED};

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    app.load_inventory();

    h1(ui, "Inventory");
    muted(ui, "Everything you own, collectibles first. Open a collectible to sell it.");
    ui.add_space(8.0);

    match app.inventory.data.clone() {
        Load::Ready(bundle) => {
            ui::wrap(ui, |ui| {
                ui::stat(ui, "OWNED ASSETS", &ui::money(bundle.owned.len() as i64), accent());
                ui::stat(
                    ui,
                    "COLLECTIBLES",
                    &ui::money(bundle.collectibles.len() as i64),
                    GOLD,
                );
                let rap: u64 = bundle.collectibles.iter().map(|item| item.rap).sum();
                ui::stat(ui, "TOTAL RAP", &ui::money(rap as i64), GOLD);
                if !app.values.items.is_empty() {
                    let value: u64 = bundle
                        .collectibles
                        .iter()
                        .map(|item| app.values.value_or(item.id, item.rap))
                        .sum();
                    ui::stat(ui, "TOTAL VALUE", &ui::money(value as i64), GOLD);
                }
            });

            // Your history from Heximons (every few minutes since it started
            // tracking you); the client's own daily record when it has none.
            let me = app.session.ready().map(|session| session.user.id);
            let tracked = match me.map(|id| app.player_stats(id, false)) {
                Some(Load::Ready(Some(stats))) if stats.points.len() > 1 => Some(stats),
                _ => None,
            };
            if let Some(stats) = tracked {
                h2(ui, "Value over time");
                let since = stats.points.first().map(|point| ui::chart::short_date((point.time / 86_400) as i64));
                muted(
                    ui,
                    &format!(
                        "From Heximons, since {} · gold is value, the accent line is RAP. Hover for any point.",
                        since.unwrap_or_default()
                    ),
                );
                ui::chart::player_history(ui, &stats.points, 170.0);
            } else if !app.value_history.is_empty() {
                h2(ui, "Value over time");
                muted(
                    ui,
                    &if app.value_history.len() == 1 {
                        "Recorded once a day while the client is open; the line fills in over the coming days.".to_string()
                    } else {
                        format!("{} days recorded · gold is Heximons value, the accent line is RAP.", app.value_history.len())
                    },
                );
                ui::chart::history(ui, &app.value_history);
            }

            h2(ui, "Collectibles");
            if bundle.collectibles.is_empty() {
                muted(ui, "No collectibles yet.");
            } else {
                let mut open: Option<(crate::api::models::Collectible, Option<String>)> = None;
                ui::wrap(ui, |ui| {
                    for item in &bundle.collectibles {
                        let icon = bundle.icons.get(&item.id).cloned();
                        let selected = app
                            .inventory
                            .resale
                            .as_ref()
                            .is_some_and(|resale| resale.user_asset_id == item.user_asset_id);
                        let card = ui::tile_button(ui, 160.0, selected, |ui| {
                            app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(136.0, 136.0));
                            ui.label(RichText::new(&item.name).font(ui::semibold(12.0)));
                            muted(
                                ui,
                                &if item.asset_type.is_empty() {
                                    "Collectible".to_string()
                                } else {
                                    item.asset_type.clone()
                                },
                            );
                            ui.horizontal(|ui| {
                                if item.rap > 0 {
                                    ui.label(
                                        RichText::new(format!("RAP {}", ui::money(item.rap as i64)))
                                            .size(12.0)
                                            .color(GOLD),
                                    );
                                }
                                if let Some(serial) = item.serial {
                                    ui::pill(ui, &format!("#{serial}"), GOLD);
                                }
                            });
                            ui::analyze::value_line(ui, &app.values, item.id);
                        });
                        if card.on_hover_text("Sell or reprice").clicked() && !selected {
                            open = Some((item.clone(), icon));
                        }
                    }
                });
                if let Some((item, icon)) = open {
                    app.open_resale(&item, icon);
                }
            }

            h2(ui, "Owned asset ids");
            muted(
                ui,
                &format!(
                    "{} ids cached from owned-asset-ids; the first 80 are listed.",
                    bundle.owned.len()
                ),
            );
            ui::wrap(ui, |ui| {
                for id in bundle.owned.iter().take(80) {
                    ui.label(RichText::new(id.to_string()).monospace().size(10.5));
                }
            });
        }
        Load::Err(error) => {
            failed(ui, &error);
            if ui.button("Retry").clicked() {
                app.inventory.data = Load::Idle;
                app.load_inventory();
            }
        }
        _ => waiting(ui, "inventory"),
    }
}

/// The Sell window for one copy: its listing, the market around it, a price box
/// and the actions. Every action goes through the confirmation dialog.
pub fn resale_window(app: &mut App, cx: &Ctx, ctx: &egui::Context) {
    // Copy out what the window shows, so drawing never holds a borrow of `app`.
    let Some(resale) = app.inventory.resale.as_ref() else {
        return;
    };
    let asset_id = resale.asset_id;
    let user_asset_id = resale.user_asset_id;
    let name = resale.name.clone();
    let serial = resale.serial;
    let fallback_rap = resale.rap;
    let icon = resale.icon.clone();
    let info = resale.info.clone();
    let mut price_input = resale.price_input.clone();

    let mut open = true;
    let mut ask: Option<WriteAction> = None;
    ui::dock(ctx, "sell", 460.0, egui::Id::new(("resale", user_asset_id)), format!("Sell · {name}"), &mut open, |ui| {
            ui.horizontal_top(|ui| {
                app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(96.0, 96.0));
                ui.vertical(|ui| {
                    ui.label(RichText::new(&name).font(ui::display(20.0)));
                    if let Some(serial) = serial {
                        ui::pill(ui, &format!("Serial #{serial}"), GOLD);
                    }
                    match &info {
                        Load::Ready(info) => {
                            let rap = if info.rap > 0 { info.rap } else { fallback_rap };
                            ui.label(
                                RichText::new(format!("RAP {} R$", ui::money(rap as i64)))
                                    .size(13.0)
                                    .color(GOLD),
                            );
                            muted(
                                ui,
                                &match info.lowest_other {
                                    Some(lowest) => format!(
                                        "Cheapest on sale: {} R$ · {} other seller(s)",
                                        ui::money(lowest as i64),
                                        info.other_sellers
                                    ),
                                    None => "Nobody else is selling one.".to_string(),
                                },
                            );
                            muted(ui, &format!("{} sold so far", ui::money(info.sales as i64)));
                        }
                        Load::Err(error) => failed(ui, error),
                        _ => waiting(ui, "the market"),
                    }
                });
            });

            let Load::Ready(info) = &info else { return };
            if let Some(history) = &info.history {
                ui.add_space(8.0);
                ui::eyebrow(ui, "Price history");
                // What is typed so far, drawn across the chart.
                let considering: Option<u64> = price_input.trim().parse().ok().filter(|price| *price > 0);
                ui::chart::show_with_price(ui, history, considering);
            }
            ui.add_space(10.0);
            ui.separator();
            match info.my_price {
                Some(price) => ui.label(
                    RichText::new(format!("On sale for {} R$", ui::money(price as i64)))
                        .font(ui::semibold(14.0))
                        .color(GREEN),
                ),
                None => ui.label(RichText::new("Not on sale").font(ui::semibold(14.0)).color(MUTED)),
            };

            ui.horizontal(|ui| {
                ui.label("Price");
                ui.add(
                    egui::TextEdit::singleline(&mut price_input)
                        .hint_text("R$")
                        .char_limit(10)
                        .desired_width(120.0),
                );
                ui.label(RichText::new("R$").color(GOLD));
            });
            // Digits only: the site rejects anything but a whole number.
            price_input.retain(|character| character.is_ascii_digit());
            let typed: Option<u64> = price_input.parse().ok().filter(|price| *price > 0);

            let action = |new: Option<u64>| WriteAction::SetResalePrice {
                asset_id,
                user_asset_id,
                name: name.clone(),
                serial,
                rap: if info.rap > 0 { info.rap } else { fallback_rap },
                lowest_other: info.lowest_other,
                old: info.my_price,
                new,
            };
            ui.add_space(4.0);
            ui.horizontal(|ui| match info.my_price {
                None => {
                    let label = match typed {
                        Some(price) => format!("List for {} R$…", ui::money(price as i64)),
                        None => "List for sale…".to_string(),
                    };
                    if ui.add_enabled(typed.is_some(), egui::Button::new(label)).clicked() {
                        ask = Some(action(typed));
                    }
                }
                Some(current) => {
                    let changed = typed.is_some_and(|price| price != current);
                    if ui.add_enabled(changed, egui::Button::new("Change price…")).clicked() {
                        ask = Some(action(typed));
                    }
                    if ui.button("Take off sale…").clicked() {
                        ask = Some(action(None));
                    }
                }
            });
            muted(ui, "You confirm on the next step.");
        });

    if let Some(resale) = app.inventory.resale.as_mut() {
        if resale.user_asset_id == user_asset_id {
            resale.price_input = price_input;
        }
    }
    if let Some(action) = ask {
        app.ask(action);
    }
    if !open {
        app.inventory.resale = None;
    }
}
