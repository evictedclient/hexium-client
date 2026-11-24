//! The Trade bot screen: start and stop, what the bot does, how it judges
//! items, the trades waiting for review, and what it has done.

use std::time::Instant;

use eframe::egui::{self, RichText, Ui};

use crate::app::{App, TradeKind, MASS_MIN_GAP};
use crate::bot::{self, BotTrade, LogKind};
use crate::config::Split;
use crate::ui::{self, accent, muted, Ctx, DANGER, GOLD, GREEN, MUTED};

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    ui::h1(ui, "Trade bot");
    muted(
        ui,
        "Runs in the background while the app is open (also when it is hidden in the tray): makes upgrades and downgrades from your items, counters inbound trades below your bar, and sends them one at a time.",
    );
    ui.add_space(8.0);
    // Filters can change on the Trade helpers screen too.
    app.sync_bot();
    status(app, ui);
    ui.add_space(10.0);
    let before = app.cfg.bot.clone();
    ui.columns(2, |columns| {
        settings_left(app, &mut columns[0]);
        settings_right(app, cx, &mut columns[1]);
    });
    if app.cfg.bot != before {
        app.cfg.save();
        app.sync_bot();
    }
    ui.add_space(10.0);
    egui::CollapsingHeader::new(RichText::new("Who to send to").strong()).id_salt("bot-who").show(ui, |ui| {
        ui::helpers::who(app, ui);
    });
    ui.add_space(10.0);
    review(app, cx, ui);
    ui.add_space(10.0);
    log(app, ui);
}

fn status(app: &mut App, ui: &mut Ui) {
    let shared = app.bot.clone();
    let (running, phase, next_round, next_send, sent, countered, failed, last_hour, rounds) = {
        let state = shared.state.lock().unwrap();
        (
            state.running,
            state.phase.clone(),
            state.next_round,
            state.next_send,
            state.sent,
            state.countered,
            state.failed,
            state.sent_last_hour(),
            state.rounds,
        )
    };
    let confirm_id = egui::Id::new("bot-confirm-auto");
    let mut confirming = ui.data(|data| data.get_temp::<bool>(confirm_id).unwrap_or(false));
    ui::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            if running {
                ui::pill(ui, "RUNNING", GREEN);
            } else {
                ui::pill(ui, "STOPPED", MUTED);
            }
            ui.label(RichText::new(if phase.is_empty() { "Not started" } else { &phase }).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if running {
                    if ui.button("Stop").clicked() {
                        shared.stop();
                    }
                    if ui.button("Run a round now").clicked() {
                        shared.run_now();
                    }
                } else if !confirming && ui.add(egui::Button::new(RichText::new("Start").strong()).fill(ui::tint(accent(), 70))).clicked() {
                    if app.cfg.bot.auto_send {
                        confirming = true;
                    } else {
                        app.start_bot();
                    }
                }
            });
        });
        if confirming {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "Auto-send is on: the bot will send up to {} trades an hour from your account without asking about each one. Start it?",
                    app.cfg.bot.per_hour.max(1)
                ))
                .color(GOLD),
            );
            ui.horizontal(|ui| {
                if ui.button("Start and send on its own").clicked() {
                    confirming = false;
                    app.start_bot();
                }
                if ui.button("Cancel").clicked() {
                    confirming = false;
                }
            });
        }
        ui.add_space(4.0);
        let mut line = format!(
            "{sent} sent Â· {countered} countered Â· {failed} failed Â· {last_hour} of {} this hour Â· {rounds} round(s)",
            app.cfg.bot.per_hour.max(1)
        );
        if running {
            if let Some(at) = next_round {
                line.push_str(&format!(" Â· next round {}", until(at)));
            }
            if let Some(at) = next_send.filter(|at| *at > Instant::now()) {
                line.push_str(&format!(" Â· next send {}", until(at)));
            }
        }
        muted(ui, &line);
    });
    ui.data_mut(|data| data.insert_temp(confirm_id, confirming));
}

fn settings_left(app: &mut App, ui: &mut Ui) {
    let bot = &mut app.cfg.bot;
    ui::eyebrow(ui, "What it does");
    ui.checkbox(&mut bot.send_new, "Make and send new trades");
    ui.checkbox(&mut bot.counter, "Counter inbound trades below my bar")
        .on_hover_text("Inbound trades already at or above your lowest gain (Other trades and counters) are pointed out in the log for you to accept; the bot never accepts or declines.");
    ui.add_space(6.0);
    kind(ui, "Upgrades", "Give two or more items for fewer, better ones.", "Most items I give", &mut bot.upgrades);
    ui.add_space(4.0);
    kind(ui, "Downgrades", "Give one big item for several smaller ones.", "Most items I get back", &mut bot.downgrades);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.radio_value(&mut bot.split, Split::Top, "Downgrade my top");
        ui.add_enabled(
            bot.split == Split::Top,
            egui::DragValue::new(&mut bot.top_items).range(1..=50),
        );
        ui.label("items, upgrade the rest");
    })
    .response
    .on_hover_text("Downgrades only give away one of your most valuable items (after the keep list); upgrades give items from everything below them.");
    ui.horizontal(|ui| {
        ui.radio_value(&mut bot.split, Split::Value, "Upgrade under");
        ui.add_enabled(
            bot.split == Split::Value,
            egui::DragValue::new(&mut bot.upgrade_to).range(1..=100_000_000).suffix(" R$"),
        );
        ui.label("value, downgrade the rest");
    })
    .response
    .on_hover_text("Splits your items by what the bot counts each one at: Heximons value, moved by the demand, sales and price settings above, and capped at the cheapest listing. Items worth less than the value are upgraded; items at or over it are given in downgrades.");
    ui.add_enabled_ui(bot.upgrades.on && bot.downgrades.on, |ui| {
        ui.horizontal(|ui| {
            ui.label("Upgrades are");
            ui.add(egui::DragValue::new(&mut bot.upgrade_share).range(0..=100).suffix("%"));
            ui.label("of each round's trades");
        });
    });
    ui.add_space(4.0);
    ui.label(RichText::new("Other trades and counters").strong());
    ui.horizontal(|ui| {
        gain(ui, &mut bot.min_gain, &mut bot.max_gain);
    });
    muted(
        ui,
        match (bot.upgrades.on, bot.downgrades.on) {
            (false, false) => "Neither kind is on: new trades are any trade in this range.",
            (true, true) => "Each round finds both kinds, split as above. This range is for counters.",
            _ => "This range is for counters.",
        },
    );
    ui.add_space(6.0);
    ui::eyebrow(ui, "How it values items");
    ui.checkbox(&mut bot.adjust, "Move values for demand, sales and price trend").on_hover_text(
        "Starting from the Heximons value:\n\
         Demand: Terrible -20%, Low -10%, Normal 0, High +5%.\n\
         Sales in the last 30 days: none -20%, 1-2 -10%, 3-9 0, 10+ +5%.\n\
         Price this week against the 3 weeks before: half of the change, from -20% to +15%.\n\
         Used on both sides, so it also prefers giving away items that sell badly.",
    );
    ui.add_enabled_ui(bot.adjust, |ui| {
        ui.horizontal(|ui| {
            ui.label("Strength");
            ui.add(egui::DragValue::new(&mut bot.adjust_strength).range(0..=300).suffix("%"));
            muted(ui, "100% as in the tooltip, 200% twice as strong");
        });
    });
    ui.checkbox(&mut bot.use_best_price, "Cap values at the cheapest listing")
        .on_hover_text("An item anyone can buy for less than its value is worth that much.");
    muted(ui, "Trades below show the adjusted values; hover an item to see why it moved.");
}

/// One kind of new trade: on/off, its gain range, its item limit.
fn kind(ui: &mut Ui, title: &str, about: &str, items_label: &str, settings: &mut crate::config::KindSettings) {
    ui.checkbox(&mut settings.on, RichText::new(title).strong()).on_hover_text(about);
    ui.add_enabled_ui(settings.on, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(22.0);
            gain(ui, &mut settings.min_gain, &mut settings.max_gain);
        });
        ui.horizontal(|ui| {
            ui.add_space(22.0);
            ui.label(items_label);
            ui.add(egui::DragValue::new(&mut settings.max_items).range(2..=crate::app::TRADE_SIDE_LIMIT));
        });
    });
}

fn gain(ui: &mut Ui, min: &mut i32, max: &mut i32) {
    ui.label("Get back");
    ui.add(egui::DragValue::new(min).range(-90..=500).suffix("%"));
    ui.label("to");
    ui.add(egui::DragValue::new(max).range(-90..=500).suffix("%"));
    ui.label("more than I give").on_hover_text("Negative numbers let you overpay, which upgrades often need.");
}

fn settings_right(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    {
        let bot = &mut app.cfg.bot;
        ui::eyebrow(ui, "Sending");
        ui.checkbox(&mut bot.auto_send, "Send on its own")
            .on_hover_text("Off: trades wait below for you to send or skip.");
        egui::Grid::new("bot-sending").num_columns(2).spacing(egui::vec2(12.0, 4.0)).show(ui, |ui| {
            ui.label("At most");
            ui.add(egui::DragValue::new(&mut bot.per_hour).range(1..=u32::MAX).suffix(" trades an hour"));
            ui.end_row();
            ui.label("Wait between trades");
            ui.add(egui::DragValue::new(&mut bot.gap_secs).range(MASS_MIN_GAP..=u64::MAX).suffix(" s"));
            ui.end_row();
            ui.label("A round every");
            ui.add(egui::DragValue::new(&mut bot.cycle_minutes).range(1..=u64::MAX).suffix(" min"));
            ui.end_row();
            ui.label("New trades per round");
            ui.add(egui::DragValue::new(&mut bot.per_cycle).range(1..=usize::MAX));
            ui.end_row();
        });
    }
    ui.add_space(6.0);
    ui::eyebrow(ui, "Never trade away");
    let Some(bundle) = app.inventory.data.ready() else {
        ui::waiting(ui, "your inventory");
        return;
    };
    let mut items: Vec<(u64, String, u64)> = Vec::new();
    for item in &bundle.collectibles {
        if !items.iter().any(|(asset, ..)| *asset == item.id) {
            items.push((item.id, item.name.clone(), app.values.value_or(item.id, item.rap)));
        }
    }
    items.sort_by(|a, b| b.2.cmp(&a.2));
    if items.is_empty() {
        muted(ui, "You have no collectibles.");
        return;
    }
    let mut toggle: Option<u64> = None;
    let mut open_item: Option<(u64, String)> = None;
    egui::ScrollArea::vertical().id_salt("bot-keep").max_height(170.0).show(ui, |ui| {
        for (asset, name, value) in &items {
            ui.horizontal(|ui| {
                let mut kept = app.cfg.bot.keep.contains(asset);
                if ui.checkbox(&mut kept, "").changed() {
                    toggle = Some(*asset);
                }
                if ui::item_card::link(ui, cx, *asset, RichText::new(name).size(12.0)).clicked() {
                    open_item = Some((*asset, name.clone()));
                }
                ui.label(RichText::new(format!("{} R$", ui::money(*value as i64))).size(11.5).color(MUTED));
            });
        }
    });
    muted(ui, &format!("{} kept", app.cfg.bot.keep.len()));
    if let Some(asset) = toggle {
        if let Some(index) = app.cfg.bot.keep.iter().position(|kept| *kept == asset) {
            app.cfg.bot.keep.remove(index);
        } else {
            app.cfg.bot.keep.push(asset);
        }
    }
    if let Some((asset, name)) = open_item {
        app.open_item(asset, &name);
    }
}

fn review(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    let shared = app.bot.clone();
    let queue: Vec<BotTrade> = shared.state.lock().unwrap().queue.clone();
    let waiting = queue.iter().filter(|trade| !trade.approved).count();
    ui.horizontal(|ui| {
        ui::eyebrow(ui, &format!("Trades ({} waiting for you, {} to send)", waiting, queue.len() - waiting));
        if waiting > 0 {
            if ui.small_button("Send all").clicked() {
                shared.approve_all();
            }
            if ui.small_button("Skip all").clicked() {
                shared.clear_queue();
            }
        }
    });
    if queue.is_empty() {
        muted(ui, "Nothing yet. Trades the bot finds show up here.");
        return;
    }
    let mut profile: Option<u64> = None;
    let mut open_item: Option<(u64, String)> = None;
    let width = (ui.available_width() - 16.0).max(320.0);
    for trade in &queue {
        ui::tile(ui, width, |ui| {
            ui.horizontal(|ui| {
                if ui::player_card::link(ui, cx, trade.partner, RichText::new(&trade.name).strong()).clicked() {
                    profile = Some(trade.partner);
                }
                ui::player_card::club_badge(ui, cx, trade.partner);
                if trade.counter_of.is_some() {
                    ui::pill(ui, "COUNTER", GOLD);
                }
                if trade.kind != TradeKind::Even {
                    ui::pill(ui, trade.kind.label(), accent());
                }
                let gain = trade.gain_pct();
                ui.label(RichText::new(format!("{gain:+.1}%")).strong().color(if gain >= 0.0 { GREEN } else { DANGER }));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if trade.approved {
                        muted(ui, "Sending soon");
                    } else {
                        if ui.small_button("Skip").clicked() {
                            shared.skip(trade.id);
                        }
                        if ui.small_button("Send").clicked() {
                            shared.approve(trade.id);
                        }
                    }
                });
            });
            side(ui, cx, "You give", &trade.give, trade.give_total(), &mut open_item);
            side(ui, cx, "You get", &trade.get, trade.get_total(), &mut open_item);
        });
    }
    if let Some(user) = profile {
        app.open_profile(user);
    }
    if let Some((asset, name)) = open_item {
        app.open_item(asset, &name);
    }
}

fn side(ui: &mut Ui, cx: &Ctx, label: &str, pieces: &[bot::Piece], total: u64, open_item: &mut Option<(u64, String)>) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(label).size(11.5).color(MUTED));
        for piece in pieces {
            let text = if piece.value == piece.base {
                format!("{} ({})", piece.name, ui::money(piece.value as i64))
            } else {
                format!("{} ({}, was {})", piece.name, ui::money(piece.value as i64), ui::money(piece.base as i64))
            };
            let response = ui::item_card::link(ui, cx, piece.asset, RichText::new(text).size(12.0));
            if !piece.why.is_empty() {
                ui.label(RichText::new("?").size(11.0).color(MUTED)).on_hover_text(format!("Counted at {}: {}", ui::money(piece.value as i64), piece.why));
            }
            if response.clicked() {
                *open_item = Some((piece.asset, piece.name.clone()));
            }
        }
        ui.label(RichText::new(format!("= {} R$", ui::money(total as i64))).size(12.0).strong().color(GOLD));
    });
}

fn log(app: &mut App, ui: &mut Ui) {
    let lines: Vec<bot::LogLine> = app.bot.state.lock().unwrap().log.iter().take(80).cloned().collect();
    ui::eyebrow(ui, "What it has done");
    if lines.is_empty() {
        muted(ui, "Nothing yet.");
        return;
    }
    for line in lines {
        let color = match line.kind {
            LogKind::Info => ui::INK,
            LogKind::Sent => GREEN,
            LogKind::Good => GOLD,
            LogKind::Problem => DANGER,
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(ago(line.at)).size(11.0).color(MUTED));
            ui.label(RichText::new(&line.text).size(12.0).color(color));
        });
    }
}

fn ago(at: Instant) -> String {
    let secs = at.elapsed().as_secs();
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", secs / 60),
        _ => format!("{} h ago", secs / 3600),
    }
}

fn until(at: Instant) -> String {
    let secs = at.saturating_duration_since(Instant::now()).as_secs();
    match secs {
        0 => "now".to_string(),
        1..=59 => format!("in {secs} s"),
        _ => format!("in {} min {} s", secs / 60, secs % 60),
    }
}
