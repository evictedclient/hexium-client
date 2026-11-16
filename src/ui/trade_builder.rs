//! The trade builder: pick up to four of your collectibles and up to four of
//! theirs, see both RAP totals, then send (or counter). Sending goes through the
//! shared confirmation dialog like every other write.

use eframe::egui::{self, RichText, Sense, Stroke, Ui};

use crate::api::models::{Collectible, TradeInventory};
use crate::app::{App, Load, TradeSide, WriteAction, TRADE_SIDE_LIMIT};
use crate::ui::{self, accent, failed, muted, waiting, Ctx, GOLD, INK, MUTED};

pub fn window(app: &mut App, cx: &Ctx, ctx: &egui::Context) {
    let Some(builder) = app.trade_builder.as_ref() else {
        return;
    };
    let title = match builder.counter_of {
        Some(_) => format!("Counter {}'s trade", builder.partner),
        None => format!("Trade with {}", builder.partner),
    };

    let mut open = true;
    let mut ask: Option<WriteAction> = None;
    egui::Window::new(RichText::new(title).font(ui::display(19.0)))
        .id(egui::Id::new("trade-builder"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size(egui::vec2(780.0, 560.0))
        .show(ctx, |ui| {
            let Some(builder) = app.trade_builder.as_mut() else { return };
            muted(
                ui,
                &format!(
                    "Pick up to {TRADE_SIDE_LIMIT} items on each side. Nothing moves until {} accepts.",
                    builder.partner
                ),
            );
            ui.add_space(6.0);

            // Both columns borrow the builder; thumbnails come from `app.thumbs`,
            // so collect clicks here and apply them after drawing.
            let mut toggle_mine: Option<u64> = None;
            let mut toggle_theirs: Option<u64> = None;
            let mine = builder.mine.clone();
            let theirs = builder.theirs.clone();
            let give = builder.give.clone();
            let get = builder.get.clone();
            let partner = builder.partner.clone();
            let mut filter_mine = builder.filter_mine.clone();
            let mut filter_theirs = builder.filter_theirs.clone();

            ui.columns(2, |columns| {
                side_column(
                    &mut columns[0],
                    app_thumbs(&mut app.thumbs),
                    cx,
                    "You give",
                    "Your collectibles",
                    &mine,
                    &give,
                    &mut filter_mine,
                    &mut toggle_mine,
                );
                side_column(
                    &mut columns[1],
                    app_thumbs(&mut app.thumbs),
                    cx,
                    "You get",
                    &format!("{partner}'s collectibles"),
                    &theirs,
                    &get,
                    &mut filter_theirs,
                    &mut toggle_theirs,
                );
            });

            let Some(builder) = app.trade_builder.as_mut() else { return };
            builder.filter_mine = filter_mine;
            builder.filter_theirs = filter_theirs;
            toggle(&mut builder.give, toggle_mine);
            toggle(&mut builder.get, toggle_theirs);

            // Totals and the send button.
            let give_side = side_of(&builder.mine, &builder.give);
            let get_side = side_of(&builder.theirs, &builder.get);
            ui.add_space(8.0);
            ui.separator();
            ui.horizontal(|ui| {
                total(ui, "You give", &give_side, builder.give.len());
                ui.add_space(24.0);
                total(ui, "You get", &get_side, builder.get.len());
                ui.add_space(24.0);
                let difference = get_side.rap as i64 - give_side.rap as i64;
                let (text, color) = match difference.cmp(&0) {
                    std::cmp::Ordering::Greater => (format!("+{} RAP for you", ui::money(difference)), ui::GREEN),
                    std::cmp::Ordering::Less => (format!("{} RAP for you", ui::money(difference)), ui::DANGER),
                    std::cmp::Ordering::Equal => ("Even on RAP".to_string(), MUTED),
                };
                ui.label(RichText::new(text).font(ui::semibold(14.0)).color(color));
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let ready = !builder.give.is_empty() && !builder.get.is_empty();
                let label = if builder.counter_of.is_some() { "Send counter…" } else { "Send trade…" };
                let button = egui::Button::new(
                    RichText::new(label).font(ui::semibold(13.5)).color(egui::Color32::from_rgb(18, 18, 20)),
                )
                .fill(if ready { accent() } else { ui::raised() });
                if ui.add_enabled(ready, button).clicked() {
                    ask = Some(WriteAction::SendTrade {
                        partner_id: builder.partner_id,
                        partner: builder.partner.clone(),
                        counter_of: builder.counter_of,
                        give_ids: builder.give.clone(),
                        get_ids: builder.get.clone(),
                        give: give_side.clone(),
                        get: get_side.clone(),
                    });
                }
                if ui.button("Clear").clicked() {
                    builder.give.clear();
                    builder.get.clear();
                }
                if !ready {
                    muted(ui, "Pick at least one item on each side.");
                } else {
                    muted(ui, "You confirm on the next step.");
                }
            });
        });

    if let Some(action) = ask {
        app.ask(action);
    }
    if !open {
        app.trade_builder = None;
    }
}

/// Lets each column borrow the thumbnail cache in turn.
fn app_thumbs(thumbs: &mut ui::Thumbs) -> &mut ui::Thumbs {
    thumbs
}

#[allow(clippy::too_many_arguments)]
fn side_column(
    ui: &mut Ui,
    thumbs: &mut ui::Thumbs,
    cx: &Ctx,
    heading: &str,
    owner_label: &str,
    inventory: &Load<TradeInventory>,
    picked: &[u64],
    filter: &mut String,
    clicked: &mut Option<u64>,
) {
    ui.horizontal(|ui| {
        ui::eyebrow(ui, heading);
        ui.label(
            RichText::new(format!("{}/{TRADE_SIDE_LIMIT}", picked.len()))
                .size(11.5)
                .color(if picked.len() >= TRADE_SIDE_LIMIT { GOLD } else { MUTED }),
        );
    });
    muted(ui, owner_label);
    ui.add(
        egui::TextEdit::singleline(filter)
            .hint_text("Search by name…")
            .desired_width(f32::INFINITY),
    );
    ui.add_space(4.0);

    match inventory {
        Load::Ready(inventory) => {
            if inventory.items.is_empty() {
                muted(ui, "No collectibles.");
                return;
            }
            let needle = filter.trim().to_lowercase();
            let full = picked.len() >= TRADE_SIDE_LIMIT;
            egui::ScrollArea::vertical()
                .id_salt(heading)
                .max_height(300.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let mut shown = 0;
                    // Picked items first, so a prefilled counter shows what is in it.
                    let ordered = inventory
                        .items
                        .iter()
                        .filter(|item| picked.contains(&item.user_asset_id))
                        .chain(inventory.items.iter().filter(|item| !picked.contains(&item.user_asset_id)));
                    for item in ordered {
                        let selected = picked.contains(&item.user_asset_id);
                        // Picked items always stay visible, whatever the search.
                        if !selected && !needle.is_empty() && !item.name.to_lowercase().contains(&needle) {
                            continue;
                        }
                        shown += 1;
                        let icon = inventory.icons.get(&item.id).cloned();
                        if item_row(ui, thumbs, cx, item, icon.as_deref(), selected, full) {
                            *clicked = Some(item.user_asset_id);
                        }
                    }
                    if shown == 0 {
                        muted(ui, "Nothing matches.");
                    }
                });
        }
        Load::Err(error) => failed(ui, &format!("Could not load collectibles: {error}")),
        _ => waiting(ui, "collectibles"),
    }
}

/// One selectable row. Returns true when clicked and the click should count:
/// unpicking always works, picking only while the side has room.
fn item_row(
    ui: &mut Ui,
    thumbs: &mut ui::Thumbs,
    cx: &Ctx,
    item: &Collectible,
    icon: Option<&str>,
    selected: bool,
    full: bool,
) -> bool {
    let available = selected || !full;
    let fill = if selected { ui::tint(accent(), 46) } else { ui::card() };
    let shown = egui::Frame::none()
        .fill(fill)
        .stroke(Stroke::new(1.0, if selected { accent() } else { ui::line() }))
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                thumbs.show(ui, cx, icon, egui::vec2(40.0, 40.0));
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(&item.name)
                            .font(ui::semibold(12.5))
                            .color(if available { INK } else { MUTED }),
                    );
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("RAP {}", ui::money(item.rap as i64)))
                                .size(11.5)
                                .color(GOLD),
                        );
                        if let Some(serial) = item.serial {
                            ui.label(RichText::new(format!("#{serial}")).size(11.0).color(MUTED));
                        }
                    });
                });
                if selected {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui::pill(ui, "PICKED", accent());
                    });
                }
            });
        });
    ui.add_space(2.0);
    let response = shown.response.interact(Sense::click());
    let response = if available {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response.on_hover_text(format!("At most {TRADE_SIDE_LIMIT} items per side"))
    };
    response.clicked() && available
}

fn toggle(picked: &mut Vec<u64>, clicked: Option<u64>) {
    let Some(id) = clicked else { return };
    if let Some(position) = picked.iter().position(|picked| *picked == id) {
        picked.remove(position);
    } else if picked.len() < TRADE_SIDE_LIMIT {
        picked.push(id);
    }
}

/// The picked copies as the confirmation dialog shows them.
fn side_of(inventory: &Load<TradeInventory>, picked: &[u64]) -> TradeSide {
    let Load::Ready(inventory) = inventory else {
        return TradeSide::default();
    };
    let items: Vec<(String, u64, Option<u64>)> = picked
        .iter()
        .filter_map(|id| inventory.items.iter().find(|item| item.user_asset_id == *id))
        .map(|item| (item.name.clone(), item.rap, item.serial))
        .collect();
    TradeSide {
        rap: items.iter().map(|(_, rap, _)| rap).sum(),
        items,
        robux: None,
    }
}

fn total(ui: &mut Ui, label: &str, side: &TradeSide, count: usize) {
    ui.vertical(|ui| {
        ui::eyebrow(ui, label);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("RAP {}", ui::money(side.rap as i64)))
                    .font(ui::display(20.0))
                    .color(GOLD),
            );
            muted(ui, &format!("{count} item(s)"));
        });
    });
}
