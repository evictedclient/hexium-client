use eframe::egui::{self, RichText, Ui};

use crate::api::endpoints::TRANSACTION_TYPES;
use crate::app::{App, Load};
use crate::ui::{self, failed, h1, h2, muted, waiting, Ctx, accent, GOLD, GREEN, MUTED};

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    app.load_money();

    h1(ui, "Money");
    muted(ui, "What you have, and where it went.");
    ui.add_space(8.0);

    if let Some(session) = app.session.ready() {
        let (robux, tix) = (session.currency.robux, session.currency.tix);
        ui::wrap(ui, |ui| {
            ui::stat(ui, "ROBUX", &ui::money(robux), GOLD);
            ui::stat(ui, "TICKETS", &ui::money(tix), GREEN);
        });
    }

    h2(ui, "Transactions");
    let current = app.money_kind();
    let mut picked: Option<&str> = None;
    ui.horizontal(|ui| {
        for (kind, label) in TRANSACTION_TYPES {
            if ui.selectable_label(current == kind, label).clicked() && current != kind {
                picked = Some(kind);
            }
        }
    });
    if let Some(kind) = picked {
        app.money.kind = kind.to_string();
        app.reload_money();
    }
    ui.add_space(4.0);

    match app.money.data.clone() {
        Load::Ready(page) => {
            if page.items.is_empty() {
                muted(ui, "Nothing of this type on the account.");
                return;
            }

            let robux: i64 = page
                .items
                .iter()
                .filter(|row| row.currency.eq_ignore_ascii_case("robux"))
                .map(|row| row.amount)
                .sum();
            let tix: i64 = page
                .items
                .iter()
                .filter(|row| !row.currency.eq_ignore_ascii_case("robux"))
                .map(|row| row.amount)
                .sum();
            muted(
                ui,
                &format!(
                    "{} row(s) loaded · {} R$ · {} Tix",
                    page.items.len(),
                    ui::money(robux),
                    ui::money(tix)
                ),
            );
            ui.add_space(4.0);

            // The item column takes whatever the fixed columns leave, so the table
            // spans the screen instead of hugging the left edge.
            let item_width = (ui.available_width() - 520.0).max(220.0);
            let mut open_profile: Option<u64> = None;
            let mut open_item: Option<(u64, String)> = None;
            egui::Grid::new("transactions")
                .striped(true)
                .num_columns(5)
                .spacing(egui::vec2(22.0, 6.0))
                .show(ui, |ui| {
                    for heading in ["DATE", "ITEM", "KIND", "WITH", "AMOUNT"] {
                        ui.label(RichText::new(heading).size(10.0).strong().color(MUTED));
                    }
                    ui.end_row();

                    for row in &page.items {
                        ui.label(
                            RichText::new(row.created.replace('T', " ").chars().take(16).collect::<String>())
                                .size(12.5)
                                .color(MUTED),
                        );

                        let name = if row.item_name.is_empty() {
                            "—"
                        } else {
                            row.item_name.as_str()
                        };
                        ui.horizontal(|ui| {
                            ui.set_min_width(item_width);
                        if row.item_id != 0 && row.item_kind == "Asset" {
                            if ui::item_card::link(ui, cx, row.item_id, RichText::new(name).size(12.0)).clicked() {
                                open_item = Some((row.item_id, row.item_name.clone()));
                            }
                        } else {
                            ui.label(RichText::new(name).size(12.0));
                        }
                        });

                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&row.item_kind).size(11.0).color(MUTED));
                            if row.pending {
                                ui::pill(ui, "PENDING", accent());
                            }
                        });

                        ui.horizontal(|ui| {
                            if let Some(icon) = page.icons.get(&row.agent_id) {
                                app.thumbs.show(ui, cx, Some(icon.as_str()), egui::vec2(20.0, 20.0));
                            }
                            if row.agent_id != 0 && !row.agent_name.is_empty() {
                                if ui::player_card::link(ui, cx, row.agent_id, RichText::new(&row.agent_name).size(12.0)).clicked() {
                                    open_profile = Some(row.agent_id);
                                }
                            } else {
                                ui.label(RichText::new(&row.agent_name).size(12.0));
                            }
                        });

                        let (unit, color) = if row.currency.eq_ignore_ascii_case("robux") {
                            ("R$", GOLD)
                        } else {
                            ("Tix", GREEN)
                        };
                        ui.label(
                            RichText::new(format!("{} {unit}", ui::money(row.amount)))
                                .size(12.0)
                                .strong()
                                .color(color),
                        );
                        ui.end_row();
                    }
                });

            ui.add_space(6.0);
            if app.money.loading_more {
                waiting(ui, "more transactions");
            } else if page.next.is_some() {
                if ui.button("Load more").clicked() {
                    app.load_more_money();
                }
            } else {
                muted(ui, "End of the list.");
            }
            if let Some(error) = &app.money.more_error {
                failed(ui, error);
            }

            if let Some((asset, name)) = open_item.take() {
                app.open_item(asset, &name);
            }
            if let Some(id) = open_profile {
                app.open_profile(id);
            }
        }
        Load::Err(error) => {
            failed(ui, &error);
            if current == "adrevenue" {
                muted(
                    ui,
                    "The site lists Ad Revenue in its own picker, but the server rejects that type.",
                );
            }
            if ui.button("Retry").clicked() {
                app.reload_money();
            }
        }
        _ => waiting(ui, "transactions"),
    }
}
