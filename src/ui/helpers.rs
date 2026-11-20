//! The Trade helpers tab on the Trades screen: say what you want, build an offer
//! (or let Suggest build it by value), see who owns it, then open a trade with
//! one owner or mass-send the offer to several. Also lists your own item values.

use eframe::egui::{self, RichText, Ui};

use crate::app::{App, Load, TRADE_SIDE_LIMIT};
use crate::ui::{self, accent, failed, muted, waiting, Ctx, GOLD, GREEN, MUTED};

/// The Trade helpers screen in the side menu.
pub fn screen(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    ui::h1(ui, "Trade helpers");
    muted(
        ui,
        "Say what you want, build an offer (or let Suggest match it by value), then trade one owner or mass-send to several.",
    );
    ui.add_space(8.0);
    show(app, cx, ui);
}

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    app.load_inventory();
    want(app, cx, ui);
    ui.add_space(10.0);
    offer(app, cx, ui);
    ui.add_space(10.0);
    who(app, ui);
    ui.add_space(10.0);
    owners(app, cx, ui);
    ui.add_space(10.0);
    range_send(app, cx, ui);
    ui.add_space(10.0);
    own_values(app, cx, ui);
}

/// Step 1: the items you want (searched from Heximons' list and your values).
fn want(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    ui::eyebrow(ui, "1 · What you want");
    let mut remove: Option<u64> = None;
    let mut open_item: Option<(u64, String)> = None;
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
                        if ui::item_card::link(ui, cx, asset, RichText::new(&name).size(12.0)).clicked() {
                            open_item = Some((asset, name.clone()));
                        }
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
                if ui::item_card::link(ui, cx, asset, &name).clicked() {
                    open_item = Some((asset, name.clone()));
                }
                ui.label(RichText::new(format!("value {}", ui::money(value as i64))).size(11.5).color(GOLD));
            });
        }
        if let Some(item) = add {
            app.trades.helper.wanted.push(item);
            app.trades.helper.search.clear();
        }
    }
    if let Some((asset, name)) = open_item {
        app.open_item(asset, &name);
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
    let mut open_item: Option<(u64, String)> = None;
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
                if ui::item_card::link(ui, cx, item.id, RichText::new(&item.name).size(12.0).color(if picked { accent() } else { ui::INK })).clicked() {
                    open_item = Some((item.id, item.name.clone()));
                }
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
    if let Some((asset, name)) = open_item {
        app.open_item(asset, &name);
    }
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
fn owners(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    ui::eyebrow(ui, "3 · Owners");
    let wanted: Vec<u64> = app.trades.helper.wanted.iter().map(|(asset, _)| *asset).collect();
    let stale = app.trades.helper.owners_for != wanted;
    let running = app.mass.as_ref().is_some_and(|mass| mass.running);
    let (cap, gap) = (app.mass_cap(), app.mass_gap());
    ui.horizontal(|ui| {
        if ui.add_enabled(!wanted.is_empty(), egui::Button::new("Find owners")).clicked() {
            app.helper_find_owners();
        }
        let ready = matches!(app.trades.helper.owners, Load::Ready(_)) && !stale && !app.trades.helper.give.is_empty() && !running;
        if ui
            .add_enabled(ready, egui::Button::new(format!("Mass send to up to {cap}…")))
            .on_hover_text(format!(
                "Sends your offer to the owners below who are not already trading with you: at most {cap}, one every {gap} seconds, after you confirm the list. Change both in Settings."
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
                        if ui::player_card::link(ui, cx, owner.user_id, &owner.name).clicked() {
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
fn own_values(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    ui::eyebrow(ui, "Your values");
    if app.cfg.value_overrides.is_empty() {
        muted(ui, "None yet. Set one from a limited's details in the Catalog; it replaces Heximons' value (or adds one) everywhere.");
        return;
    }
    let mut change: Option<(u64, String, Option<u64>)> = None;
    let mut open_item: Option<(u64, String)> = None;
    for own in app.cfg.value_overrides.clone() {
        ui.horizontal(|ui| {
            if ui::item_card::link(ui, cx, own.asset_id, &own.name).clicked() {
                open_item = Some((own.asset_id, own.name.clone()));
            }
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
    if let Some((asset, name)) = open_item {
        app.open_item(asset, &name);
    }
}

/// Step 4: a value range instead of specific items. Generates one trade per
/// player (their items adding up to the range), for you to review and send.
fn range_send(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    ui::eyebrow(ui, "4 · Or ask for a value range");
    muted(
        ui,
        "Uses your offer from step 2. Finds players with valued limiteds around the range and, for each, the combination of up to four of their items that adds up to it. Nothing is sent until you review the list and confirm.",
    );
    let generating = matches!(app.trades.helper.proposals, Load::Busy);
    let running = app.mass.as_ref().is_some_and(|mass| mass.running);
    ui.horizontal(|ui| {
        ui.label("Ask for value");
        app.trades.helper.range_min.retain(|character| character.is_ascii_digit());
        app.trades.helper.range_max.retain(|character| character.is_ascii_digit());
        ui.add(egui::TextEdit::singleline(&mut app.trades.helper.range_min).hint_text("min").char_limit(9).desired_width(80.0));
        ui.label("to");
        ui.add(egui::TextEdit::singleline(&mut app.trades.helper.range_max).hint_text("max").char_limit(9).desired_width(80.0));
        ui.label(RichText::new("R$").color(GOLD));
        let ready = app.helper_range().is_some() && !app.trades.helper.give.is_empty() && !app.values.items.is_empty() && !generating;
        if ui
            .add_enabled(ready, egui::Button::new(if generating { "Generating…" } else { "Generate trades" }))
            .on_hover_text(format!("Proposes up to {} trades (your Mass send limit).", app.mass_cap()))
            .clicked()
        {
            app.helper_generate();
        }
    });
    // Quick ranges around what you give (items by value, plus Robux).
    let give_total = app.helper_give_side().value + app.trades.helper.give_robux.parse::<u64>().unwrap_or(0);
    ui.horizontal(|ui| {
        muted(ui, "Quick");
        for (label, percent) in [("-10%", -10i64), ("-5%", -5), ("Equal", 0), ("+5%", 5), ("+10%", 10), ("+20%", 20)] {
            let (min, max) = preset_range(give_total, percent);
            let tip = match percent {
                0 => format!("About what you give: {} to {} R$ of value.", ui::money(min as i64), ui::money(max as i64)),
                p if p > 0 => format!("About {p}% more than you give: {} to {} R$ of value.", ui::money(min as i64), ui::money(max as i64)),
                p => format!("About {}% less than you give: {} to {} R$ of value.", -p, ui::money(min as i64), ui::money(max as i64)),
            };
            let active = app.helper_range() == Some((min, max));
            let button = egui::Button::new(RichText::new(label).size(12.0)).selected(active);
            if ui
                .add_enabled(give_total > 0, button)
                .on_hover_text(tip)
                .on_disabled_hover_text("Pick what you offer in step 2 first.")
                .clicked()
            {
                app.trades.helper.range_min = min.to_string();
                app.trades.helper.range_max = max.to_string();
            }
        }
        if give_total > 0 {
            muted(ui, &format!("· you give {} R$", ui::money(give_total as i64)));
        }
    });
    // Upgrade / downgrade, where they can apply to your offer.
    let my_count = app.trades.helper.give.len();
    let can_up = crate::app::RangeRules::upgrade_possible(my_count);
    let can_down = crate::app::RangeRules::downgrade_possible(my_count);
    if !can_up {
        app.trades.helper.range_upgrade = false;
    }
    if !can_down {
        app.trades.helper.range_downgrade = false;
    }
    ui.horizontal(|ui| {
        ui.add_enabled(can_up, egui::Checkbox::new(&mut app.trades.helper.range_upgrade, "Upgrade"))
            .on_hover_text("Fewer items back than you give, and their best item worth more than yours (e.g. your 3 items for 1 better one).")
            .on_disabled_hover_text("Needs at least 2 items in your offer.");
        ui.add_enabled(can_down, egui::Checkbox::new(&mut app.trades.helper.range_downgrade, "Downgrade"))
            .on_hover_text("More items back than you give, each worth less than your best (e.g. 1 big item for several smaller ones).")
            .on_disabled_hover_text("Needs 1 to 3 items in your offer (a side holds at most 4).");
        muted(
            ui,
            match (app.trades.helper.range_upgrade, app.trades.helper.range_downgrade) {
                (false, false) => "Neither ticked: any trade in the range.",
                (true, false) => "Only upgrades.",
                (false, true) => "Only downgrades.",
                (true, true) => "Upgrades or downgrades, no even swaps.",
            },
        );
    });
    if app.trades.helper.give.is_empty() {
        muted(ui, "Pick what you offer in step 2 first.");
    } else if !app.trades.helper.range_min.is_empty() && !app.trades.helper.range_max.is_empty() && app.helper_range().is_none() {
        ui.label(RichText::new("The minimum has to be above 0 and not above the maximum.").size(11.5).color(ui::DANGER));
    }

    let mut toggle: Option<usize> = None;
    let mut all: Option<bool> = None;
    let mut profile: Option<u64> = None;
    let mut send = false;
    let mut open_item: Option<(u64, String)> = None;
    let mut hovered: Option<usize> = None;
    let focus = app.trades.helper.range_focus;
    let preview_width = (ui.available_width() * 0.36).clamp(240.0, 340.0);
    let list_width = (ui.available_width() - preview_width - 14.0).max(260.0);
    ui.horizontal_top(|ui| {
    ui.vertical(|ui| {
    ui.set_width(list_width);
    match &app.trades.helper.proposals {
        Load::Busy => {
            let (done, total) = app.trades.helper.range_progress;
            waiting(ui, &if total > 0 { format!("players ({done} of {total} checked)") } else { "players to check".to_string() });
        }
        Load::Err(error) => failed(ui, &format!("Could not generate trades: {error}")),
        Load::Idle => {}
        Load::Ready(proposals) => {
            if proposals.is_empty() {
                muted(ui, "No player had items adding up to that range. Try a wider range.");
            } else {
                let picked = proposals.iter().filter(|proposal| proposal.selected).count();
                ui.horizontal(|ui| {
                    muted(ui, &format!("{} trade(s) proposed · {picked} ticked", proposals.len()));
                    if ui.small_button("All").clicked() {
                        all = Some(true);
                    }
                    if ui.small_button("None").clicked() {
                        all = Some(false);
                    }
                });
                egui::ScrollArea::vertical().id_salt("range-proposals").max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
                    for (index, proposal) in proposals.iter().enumerate() {
                        let top = ui.horizontal(|ui| {
                            let mut ticked = proposal.selected;
                            if ui.checkbox(&mut ticked, "").changed() {
                                toggle = Some(index);
                            }
                            if ui::player_card::link(ui, cx, proposal.user_id, RichText::new(&proposal.name).font(ui::semibold(12.5))).clicked() {
                                profile = Some(proposal.user_id);
                            }
                            ui.label(
                                RichText::new(format!("value {}", ui::money(proposal.total as i64)))
                                    .size(12.0)
                                    .color(GOLD),
                            );
                            match proposal.kind {
                                crate::app::TradeKind::Upgrade => ui::pill(ui, "UPGRADE", GREEN),
                                crate::app::TradeKind::Downgrade => ui::pill(ui, "DOWNGRADE", accent()),
                                crate::app::TradeKind::Even => {}
                            }
                        });
                        let bottom = ui.horizontal_wrapped(|ui| {
                            ui.add_space(26.0);
                            for (_, asset, name, value) in &proposal.items {
                                if ui::item_card::link(ui, cx, *asset, RichText::new(format!("{name} ({})", ui::money(*value as i64))).size(11.5)).clicked() {
                                    open_item = Some((*asset, name.clone()));
                                }
                            }
                        }).response;
                        let rect = top.response.rect.union(bottom.rect);
                        if ui.rect_contains_pointer(rect) {
                            hovered = Some(index);
                        }
                        if focus == Some(index) {
                            ui.painter().rect_stroke(rect.expand(2.0), 4.0, egui::Stroke::new(1.0, accent()));
                        }
                    }
                });
                ui.horizontal(|ui| {
                    let label = format!("Send selected ({})…", picked.min(app.mass_cap()));
                    send = ui
                        .add_enabled(picked > 0 && !running, egui::Button::new(label))
                        .on_hover_text(format!(
                            "Asks once, listing every trade, then sends one every {} seconds (Settings > Mass send).",
                            app.mass_gap()
                        ))
                        .clicked();
                    if picked > app.mass_cap() {
                        muted(ui, &format!("Only the first {} go out (your Mass send limit).", app.mass_cap()));
                    }
                    if running {
                        muted(ui, "A mass send is under way.");
                    }
                });
            }
        }
    }
    });
    ui.add_space(10.0);
    ui.vertical(|ui| {
        ui.set_width(preview_width);
        preview(app, cx, ui, hovered.or(focus), &mut open_item);
    });
    });
    if hovered.is_some() {
        app.trades.helper.range_focus = hovered;
    }
    if let Some((asset, name)) = open_item {
        app.open_item(asset, &name);
    }
    if let Load::Ready(proposals) = &mut app.trades.helper.proposals {
        if let Some(index) = toggle {
            if let Some(proposal) = proposals.get_mut(index) {
                proposal.selected = !proposal.selected;
            }
        }
        if let Some(on) = all {
            for proposal in proposals.iter_mut() {
                proposal.selected = on;
            }
        }
    }
    if let Some(id) = profile {
        app.open_profile(id);
    }
    if send {
        app.helper_send_proposals();
    }
}

/// Which players the helpers pick: filters and order (saved; 0 = off).
fn who(app: &mut App, ui: &mut Ui) {
    ui::eyebrow(ui, "Who to send to");
    muted(ui, "Applies to Find owners, mass send and value-range trades. 0 turns a filter off.");
    let mut filters = app.cfg.player_filters;
    let number = |ui: &mut Ui, value: &mut u32, label: &str, unit: &str| -> bool {
        ui.horizontal(|ui| {
            ui.label(label);
            let changed = ui.add(egui::DragValue::new(value).range(0..=3650).suffix(unit)).changed();
            if *value == 0 {
                ui.label(RichText::new("off").size(11.0).color(MUTED));
            }
            changed
        })
        .inner
    };
    let mut changed = false;
    egui::Grid::new("player-filters").num_columns(2).spacing(egui::vec2(24.0, 4.0)).show(ui, |ui| {
        changed |= number(ui, &mut filters.min_account_days, "Account at least", " days old");
        changed |= number(ui, &mut filters.active_within_days, "Online in the last", " days");
        ui.end_row();
        changed |= number(ui, &mut filters.min_held_days, "Has held the item at least", " days");
        changed |= number(ui, &mut filters.max_held_days, "and at most", " days");
        ui.end_row();
        changed |= number(ui, &mut filters.skip_recent_days, "Skip people I sent to in the last", " days");
        ui.horizontal(|ui| {
            ui.label("Order");
            for (value, label) in [(2u8, "Random"), (0, "Newest holders"), (1, "Longest holders")] {
                if ui.selectable_label(filters.order == value, label).clicked() && filters.order != value {
                    filters.order = value;
                    changed = true;
                }
            }
        });
        ui.end_row();
    });
    let recent = app.recent_targets().len();
    ui.horizontal(|ui| {
        if filters.skip_recent_days > 0 {
            muted(ui, &format!("{recent} recent recipient(s) skipped right now."));
        }
        if !app.cfg.recent_targets.is_empty() && ui.small_button("Forget who I sent to").clicked() {
            app.cfg.recent_targets.clear();
            app.cfg.save();
        }
    });
    if filters.min_account_days > 0 {
        muted(ui, "Account age needs one profile lookup per player, so at most 60 players are checked for it.");
    }
    if changed {
        app.cfg.player_filters = filters;
        app.cfg.save();
        // Owners found under the old filters are out of date.
        app.trades.helper.owners = Load::Idle;
    }
}

/// A range centred `percent` above (or below) what you give, 2.5% either side.
fn preset_range(give: u64, percent: i64) -> (u64, u64) {
    let give = give as f64;
    let target = give * (1.0 + percent as f64 / 100.0);
    let band = give * 0.025;
    let min = (target - band).round().max(1.0) as u64;
    let max = ((target + band).round() as u64).max(min);
    (min, max)
}

/// The trade preview beside the generated trades: what you give, what the
/// hovered (or first ticked) trade gets you, and the verdict.
fn preview(app: &App, cx: &Ctx, ui: &mut Ui, focus: Option<usize>, open_item: &mut Option<(u64, String)>) {
    ui::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui::eyebrow(ui, "Trade preview");
        let robux: u64 = app.trades.helper.give_robux.parse().unwrap_or(0);
        let copies: Vec<&crate::api::models::Collectible> = match app.inventory.data.ready() {
            Some(bundle) => app
                .trades
                .helper
                .give
                .iter()
                .filter_map(|id| bundle.collectibles.iter().find(|item| item.user_asset_id == *id))
                .collect(),
            None => Vec::new(),
        };
        let give_value: u64 = copies.iter().map(|item| app.values.value_or(item.id, item.rap)).sum();
        let give_rap: u64 = copies.iter().map(|item| item.rap).sum();

        ui.label(RichText::new("You give").font(ui::semibold(13.0)));
        if copies.is_empty() && robux == 0 {
            muted(ui, "Nothing yet: pick items in step 2.");
        }
        for item in &copies {
            row(ui, cx, item.id, &item.name, app.values.value_or(item.id, item.rap), open_item);
        }
        if robux > 0 {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Robux").size(12.0));
                amount_right(ui, robux);
            });
        }
        total(ui, give_value + robux, give_rap);

        ui.add_space(4.0);
        ui.separator();
        let proposal = match &app.trades.helper.proposals {
            Load::Ready(proposals) => focus
                .and_then(|index| proposals.get(index))
                .or_else(|| proposals.iter().find(|proposal| proposal.selected))
                .or_else(|| proposals.first()),
            _ => None,
        };
        match proposal {
            Some(proposal) => {
                ui.label(RichText::new(format!("You get from {}", proposal.name)).font(ui::semibold(13.0)));
                let mut get_rap = 0u64;
                for (_, asset, name, value) in &proposal.items {
                    row(ui, cx, *asset, name, *value, open_item);
                    get_rap += app.values.get(*asset).map(|item| item.rap).unwrap_or(0);
                }
                total(ui, proposal.total, get_rap);
                ui.add_space(4.0);
                ui::analyze::verdict(
                    ui,
                    ui::analyze::Side { rap: give_rap, value: give_value, robux },
                    ui::analyze::Side { rap: get_rap, value: proposal.total, robux: 0 },
                    &[],
                );
                muted(ui, "Hover a trade on the left to preview it.");
            }
            None => {
                ui.label(RichText::new("You get").font(ui::semibold(13.0)));
                let giving = give_value + robux;
                match app.helper_range() {
                    Some((min, max)) if giving > 0 => {
                        let percent = |amount: u64| (amount as f64 - giving as f64) / giving as f64 * 100.0;
                        muted(
                            ui,
                            &format!(
                                "{} to {} R$ of value ({:+.0}% to {:+.0}% of what you give).",
                                ui::money(min as i64),
                                ui::money(max as i64),
                                percent(min),
                                percent(max)
                            ),
                        );
                    }
                    _ => muted(ui, "Set a range, then generate trades to preview them here."),
                }
            }
        }
    });
}

fn row(ui: &mut Ui, cx: &Ctx, asset: u64, name: &str, value: u64, open_item: &mut Option<(u64, String)>) {
    ui.horizontal(|ui| {
        if ui::item_card::link(ui, cx, asset, RichText::new(name).size(12.0)).clicked() {
            *open_item = Some((asset, name.to_string()));
        }
        amount_right(ui, value);
    });
}

fn amount_right(ui: &mut Ui, value: u64) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(RichText::new(ui::money(value as i64)).size(12.0).color(GOLD));
    });
}

fn total(ui: &mut Ui, value: u64, rap: u64) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Total").size(12.0).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(format!("RAP {}", ui::money(rap as i64))).size(11.5).color(MUTED));
            ui.label(RichText::new(format!("{} R$", ui::money(value as i64))).size(12.0).strong().color(GOLD));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::preset_range;

    #[test]
    fn presets_sit_around_the_offer() {
        assert_eq!(preset_range(1000, 0), (975, 1025));
        assert_eq!(preset_range(1000, 10), (1075, 1125));
        assert_eq!(preset_range(1000, -5), (925, 975));
        // Never asks for nothing.
        assert_eq!(preset_range(10, -100).0, 1);
    }
}
