//! The Trade helpers tab on the Trades screen: say what you want, build an offer
//! (or let Suggest build it by value), see who owns it, then open a trade with
//! one owner or mass-send the offer to several. Also lists your own item values.

use eframe::egui::{self, RichText, Ui};

use crate::app::{App, Load, MASS_CAP, MASS_GAP_SECS, TRADE_SIDE_LIMIT};
use crate::ui::{self, accent, failed, muted, waiting, Ctx, GOLD, GREEN, MUTED};

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    app.load_inventory();
    want(app, cx, ui);
    ui.add_space(10.0);
    offer(app, cx, ui);
    ui.add_space(10.0);
    owners(app, ui);
    ui.add_space(10.0);
    own_values(app, ui);
}

/// Step 1: the items you want (searched from Heximons' list and your values).
fn want(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    ui::eyebrow(ui, "1 · What you want");
    let mut remove: Option<u64> = None;
    ui.horizontal_wrapped(|ui| {
        if app.trades.helper.wanted.is_empty() {
            muted(ui, "Nothing yet — search below.");
        }
        for (asset, name) in app.trades.helper.wanted.clone() {
            let value = app.values.get(asset).map(|item| item.value).unwrap_or(0);
            egui::Frame::none()
                .fill(ui::tint(accent(), 40))
                .rounding(egui::Rounding::same(12.0))
                .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let thumb = app.values.get(asset).and_then(|item| item.thumb.clone());
                        app.thumbs.show(ui, cx, thumb.as_deref(), egui::vec2(20.0, 20.0));
                        ui.label(RichText::new(&name).size(12.0));
                        if value > 0 {
                            ui.label(RichText::new(ui::money(value as i64)).size(11.0).color(GOLD));
                        }
                        if ui.small_button("✕").clicked() {
                            remove = Some(asset);
                        }
                    });
                });
        }
    });
    if let Some(asset) = remove {
        app.trades.helper.wanted.retain(|(id, _)| *id != asset);
    }
    let full = app.trades.helper.wanted.len() >= TRADE_SIDE_LIMIT;
    ui.add_enabled(
        !full,
        egui::TextEdit::singleline(&mut app.trades.helper.search)
            .hint_text(if full { "Four items at most" } else { "Search limiteds to add…" })
            .desired_width(320.0),
    );
    let needle = app.trades.helper.search.trim().to_lowercase();
    if needle.len() >= 2 && !full {
        let mut matches: Vec<(u64, String, u64)> = app
            .values
            .items
            .iter()
            .filter(|(asset, item)| {
                !app.trades.helper.wanted.iter().any(|(id, _)| id == *asset)
                    && (item.name.to_lowercase().contains(&needle)
                        || item.acronym.as_deref().is_some_and(|acronym| acronym.to_lowercase() == needle))
            })
            .map(|(asset, item)| (*asset, item.name.clone(), item.value))
            .collect();
        matches.sort_by(|a, b| b.2.cmp(&a.2));
        let mut add: Option<(u64, String)> = None;
        for (asset, name, value) in matches.into_iter().take(8) {
            ui.horizontal(|ui| {
                if ui.button("Add").clicked() {
                    add = Some((asset, name.clone()));
                }
                ui.label(&name);
                ui.label(RichText::new(format!("value {}", ui::money(value as i64))).size(11.5).color(GOLD));
            });
        }
        if let Some(item) = add {
            app.trades.helper.wanted.push(item);
            app.trades.helper.search.clear();
        }
    }
}

/// Step 2: your offer, picked by hand or suggested by value.
fn offer(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    ui::eyebrow(ui, "2 · What you offer");
    let Some(bundle) = app.inventory.data.ready().cloned() else {
        waiting(ui, "your collectibles");
        return;
    };
    let wanted_value: u64 = app
        .trades
        .helper
        .wanted
        .iter()
        .map(|(asset, _)| app.values.get(*asset).map(|item| item.value.max(item.rap)).unwrap_or(0))
        .sum();
    let robux: u64 = app.trades.helper.give_robux.parse().unwrap_or(0);
    let give_value: u64 = app
        .trades
        .helper
        .give
        .iter()
        .filter_map(|id| bundle.collectibles.iter().find(|item| item.user_asset_id == *id))
        .map(|item| app.values.value_or(item.id, item.rap))
        .sum();
    let give_rap: u64 = app
        .trades
        .helper
        .give
        .iter()
        .filter_map(|id| bundle.collectibles.iter().find(|item| item.user_asset_id == *id))
        .map(|item| item.rap)
        .sum();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!app.trades.helper.wanted.is_empty(), egui::Button::new("Suggest"))
            .on_hover_text("Picks up to four of your items whose value comes closest to what you want (Robux counted).")
            .clicked()
        {
            app.helper_suggest();
        }
        if ui.button("Clear").clicked() {
            app.trades.helper.give.clear();
        }
        ui.label("+ Robux");
        app.trades.helper.give_robux.retain(|character| character.is_ascii_digit());
        ui.add(egui::TextEdit::singleline(&mut app.trades.helper.give_robux).hint_text("0").char_limit(9).desired_width(70.0));
    });
    if !app.trades.helper.wanted.is_empty() {
        let wanted_rap: u64 = app
            .trades
            .helper
            .wanted
            .iter()
            .map(|(asset, _)| app.values.get(*asset).map(|item| item.rap).unwrap_or(0))
            .sum();
        ui::analyze::verdict(
            ui,
            ui::analyze::Side { rap: give_rap, value: give_value, robux },
            ui::analyze::Side { rap: wanted_rap, value: wanted_value, robux: 0 },
            &[],
        );
    }
    let full = app.trades.helper.give.len() >= TRADE_SIDE_LIMIT;
    let mut toggle: Option<u64> = None;
    egui::ScrollArea::vertical().id_salt("helper-offer").max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
        let mut items = bundle.collectibles.clone();
        items.sort_by(|a, b| app.values.value_or(b.id, b.rap).cmp(&app.values.value_or(a.id, a.rap)));
        for item in &items {
            let picked = app.trades.helper.give.contains(&item.user_asset_id);
            ui.horizontal(|ui| {
                let mut checked = picked;
                if ui.add_enabled(picked || !full, egui::Checkbox::new(&mut checked, "")).changed() {
                    toggle = Some(item.user_asset_id);
                }
                let icon = bundle.icons.get(&item.id).cloned();
                app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(24.0, 24.0));
                ui.label(RichText::new(&item.name).size(12.0).color(if picked { accent() } else { ui::INK }));
                if let Some(serial) = item.serial {
                    ui.label(RichText::new(format!("#{serial}")).size(11.0).color(MUTED));
                }
                ui.label(
                    RichText::new(format!("value {}", ui::money(app.values.value_or(item.id, item.rap) as i64)))
                        .size(11.0)
                        .color(GOLD),
                );
            });
        }
    });
    if let Some(id) = toggle {
        let give = &mut app.trades.helper.give;
        match give.iter().position(|picked| *picked == id) {
            Some(position) => {
                give.remove(position);
            }
            None if give.len() < TRADE_SIDE_LIMIT => give.push(id),
            None => {}
        }
    }
}

/// Step 3: who owns every wanted item; trade one, or mass-send to several.
fn owners(app: &mut App, ui: &mut Ui) {
    ui::eyebrow(ui, "3 · Owners");
    let wanted: Vec<u64> = app.trades.helper.wanted.iter().map(|(asset, _)| *asset).collect();
    let stale = app.trades.helper.owners_for != wanted;
    let running = app.mass.as_ref().is_some_and(|mass| mass.running);
    ui.horizontal(|ui| {
        if ui.add_enabled(!wanted.is_empty(), egui::Button::new("Find owners")).clicked() {
            app.helper_find_owners();
        }
        let ready = matches!(app.trades.helper.owners, Load::Ready(_)) && !stale && !app.trades.helper.give.is_empty() && !running;
        if ui
            .add_enabled(ready, egui::Button::new(format!("Mass send to up to {MASS_CAP}…")))
            .on_hover_text(format!(
                "Sends your offer to the owners below who are not already trading with you: at most {MASS_CAP}, one every {MASS_GAP_SECS} seconds, after you confirm the list."
            ))
            .clicked()
        {
            app.helper_mass_send();
        }
        if running {
            muted(ui, "A mass send is under way (see the top of the window).");
        }
    });
    if stale && !matches!(app.trades.helper.owners, Load::Idle) {
        muted(ui, "The wanted items changed; find owners again.");
        return;
    }
    match app.trades.helper.owners.clone() {
        Load::Ready(list) => {
            if list.is_empty() {
                muted(ui, "Nobody else owns all of these.");
                return;
            }
            let free = list.iter().filter(|owner| !owner.busy).count();
            muted(ui, &format!("{} owner(s) · {free} not already trading with you", list.len()));
            let mut open: Option<crate::app::Candidate> = None;
            let mut profile: Option<u64> = None;
            egui::ScrollArea::vertical().id_salt("helper-owners").max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
                for owner in &list {
                    ui.horizontal(|ui| {
                        if ui.link(&owner.name).clicked() {
                            profile = Some(owner.user_id);
                        }
                        let since: String = owner.updated.chars().take(10).collect();
                        ui.label(RichText::new(format!("since {since}")).size(11.0).color(MUTED));
                        if owner.busy {
                            ui::pill(ui, "TRADING WITH YOU", GOLD);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Open trade…").clicked() {
                                open = Some(owner.clone());
                            }
                        });
                    });
                }
            });
            if let Some(id) = profile {
                app.open_profile(id);
            }
            if let Some(owner) = open {
                app.helper_open_trade(&owner);
            }
        }
        Load::Err(error) => failed(ui, &format!("Could not find owners: {error}")),
        Load::Busy => waiting(ui, "owners"),
        Load::Idle => muted(ui, "Pick what you want, then find who owns it."),
    }
}

/// Your own values, editable here as well as in each item's details.
fn own_values(app: &mut App, ui: &mut Ui) {
    ui::eyebrow(ui, "Your values");
    if app.cfg.value_overrides.is_empty() {
        muted(ui, "None yet. Set one from a limited's details in the Catalog; it replaces Heximons' value (or adds one) everywhere.");
        return;
    }
    let mut change: Option<(u64, String, Option<u64>)> = None;
    for own in app.cfg.value_overrides.clone() {
        ui.horizontal(|ui| {
            ui.label(&own.name);
            ui.label(RichText::new(format!("{} R$", ui::money(own.value as i64))).color(accent()));
            if let Some(theirs) = app.values.get(own.asset_id).and_then(|item| item.heximons_value) {
                let difference = own.value as i64 - theirs as i64;
                ui.label(
                    RichText::new(format!("Heximons {} ({}{})", ui::money(theirs as i64), if difference > 0 { "+" } else { "" }, ui::money(difference)))
                        .size(11.0)
                        .color(if difference >= 0 { GREEN } else { ui::DANGER }),
                );
            } else {
                ui.label(RichText::new("not on Heximons").size(11.0).color(MUTED));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Clear").clicked() {
                    change = Some((own.asset_id, own.name.clone(), None));
                }
            });
        });
    }
    if let Some((asset, name, value)) = change {
        app.set_override(asset, &name, value);
    }
}
