//! The Resale bot screen: start and stop it, how far under the market to list,
//! what it has priced, and what it has done.

use std::time::Instant;

use eframe::egui::{self, RichText, Ui};

use crate::app::App;
use crate::bot::LogKind;
use crate::resale::Listing;
use crate::ui::{self, accent, muted, Ctx, DANGER, GOLD, GREEN, MUTED};

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    ui::h1(ui, "Resale bot");
    muted(
        ui,
        "Runs while the app is open: every round it reads your collectibles and lists each copy just under the cheapest other listing, repricing it when someone undercuts you. It only sets prices — it never buys and never takes a copy off sale.",
    );
    ui.add_space(8.0);
    app.sync_resale();
    status(app, ui);
    ui.add_space(10.0);
    let before = app.cfg.resale.clone();
    settings(app, ui);
    if app.cfg.resale != before {
        app.cfg.save();
        app.sync_resale();
    }
    ui.add_space(10.0);
    recent(app, cx, ui);
    ui.add_space(10.0);
    log(app, ui);
}

fn status(app: &mut App, ui: &mut Ui) {
    let shared = app.resale.clone();
    let (running, phase, next_round, listed, repriced, unchanged, skipped, failed, rounds) = {
        let state = shared.state.lock().unwrap();
        (
            state.running,
            state.phase.clone(),
            state.next_round,
            state.listed,
            state.repriced,
            state.unchanged,
            state.skipped,
            state.failed,
            state.rounds,
        )
    };
    let confirm_id = egui::Id::new("resale-confirm-start");
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
                } else if !confirming
                    && ui
                        .add(egui::Button::new(RichText::new("Start").strong()).fill(ui::tint(accent(), 70)))
                        .clicked()
                {
                    confirming = true;
                }
            });
        });
        if confirming {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "The bot will set the price of every collectible you own to its cheapest other listing minus {} R$, without asking about each one. Start it?",
                    app.cfg.resale.undercut.max(1)
                ))
                .color(GOLD),
            );
            ui.horizontal(|ui| {
                if ui.button("Start and list on its own").clicked() {
                    confirming = false;
                    app.start_resale_bot();
                }
                if ui.button("Cancel").clicked() {
                    confirming = false;
                }
            });
        }
        ui.add_space(4.0);
        let mut line = format!(
            "{listed} listed · {repriced} repriced · {unchanged} already lowest · {skipped} skipped · {failed} failed · {rounds} round(s)"
        );
        if running {
            if let Some(at) = next_round {
                line.push_str(&format!(" · next round {}", until(at)));
            }
        }
        muted(ui, &line);
    });
    ui.data_mut(|data| data.insert_temp(confirm_id, confirming));
}

fn settings(app: &mut App, ui: &mut Ui) {
    let settings = &mut app.cfg.resale;
    ui::eyebrow(ui, "How it lists");
    egui::Grid::new("resale-settings").num_columns(2).spacing(egui::vec2(12.0, 6.0)).show(ui, |ui| {
        ui.label("List");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut settings.undercut).range(1..=10_000).suffix(" R$"));
            ui.label("under the cheapest other listing");
        });
        ui.end_row();
        ui.label("Never below");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut settings.min_price).range(1..=u64::MAX).suffix(" R$"));
            muted(ui, "nobody is undercut past this");
        });
        ui.end_row();
        ui.label("A round every");
        ui.add(egui::DragValue::new(&mut settings.cycle_minutes).range(1..=u64::MAX).suffix(" min"));
        ui.end_row();
        ui.label("Wait between listings");
        ui.add(egui::DragValue::new(&mut settings.gap_secs).range(1..=u64::MAX).suffix(" s"));
        ui.end_row();
    });
    ui.checkbox(&mut settings.skip_kept, "Leave out items on the Trade bot's keep list")
        .on_hover_text("The keep list is edited on the Trade bot screen.");
    muted(
        ui,
        "A copy nobody else is selling is left alone: there is no price to go under. Copies of yours that are already the cheapest stay where they are.",
    );
}

fn recent(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    let rows: Vec<Listing> = app.resale.state.lock().unwrap().recent.iter().cloned().collect();
    ui::eyebrow(ui, "What it has priced");
    if rows.is_empty() {
        muted(ui, "Nothing yet. Prices the bot sets show up here.");
        return;
    }
    let mut open_item: Option<(u64, String)> = None;
    let width = (ui.available_width() - 16.0).max(320.0);
    for row in &rows {
        ui::tile(ui, width, |ui| {
            ui.horizontal(|ui| {
                if ui::item_card::link(ui, cx, row.asset, RichText::new(&row.name).strong()).clicked() {
                    open_item = Some((row.asset, row.name.clone()));
                }
                ui.label(
                    RichText::new(format!("{} R$", ui::money(row.price as i64)))
                        .strong()
                        .color(GREEN),
                );
                match row.previous {
                    Some(previous) => muted(ui, &format!("was {} R$", ui::money(previous as i64))),
                    None => muted(ui, "was not on sale"),
                }
                ui.label(
                    RichText::new(format!("under {}", ui::money(row.lowest_other as i64)))
                        .size(11.5)
                        .color(MUTED),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    muted(ui, &ago(row.at));
                });
            });
        });
    }
    if let Some((asset, name)) = open_item {
        app.open_item(asset, &name);
    }
}

fn log(app: &mut App, ui: &mut Ui) {
    let lines: Vec<crate::bot::LogLine> =
        app.resale.state.lock().unwrap().log.iter().take(80).cloned().collect();
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
