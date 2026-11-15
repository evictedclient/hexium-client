use eframe::egui::{self, RichText, Ui};

use crate::app::{App, Load};
use crate::ui::{self, failed, h1, h2, muted, waiting, Ctx, accent, GOLD};

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    app.load_inventory();

    h1(ui, "Inventory");
    muted(ui, "Everything you own, collectibles first.");
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
            });

            h2(ui, "Collectibles");
            if bundle.collectibles.is_empty() {
                muted(ui, "No collectibles yet.");
            } else {
                ui::wrap(ui, |ui| {
                    for item in &bundle.collectibles {
                        let icon = bundle.icons.get(&item.id).cloned();
                        ui::tile(ui, 160.0, |ui| {
                            app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(136.0, 136.0));
                            ui.label(RichText::new(&item.name).size(11.5).strong());
                            muted(
                                ui,
                                &if item.asset_type.is_empty() {
                                    "collectible".to_string()
                                } else {
                                    item.asset_type.clone()
                                },
                            );
                            if let Some(serial) = item.serial {
                                ui::pill(ui, &format!("#{serial}"), GOLD);
                            }
                        });
                    }
                });
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
