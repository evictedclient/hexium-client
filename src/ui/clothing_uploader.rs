use eframe::egui::{self, RichText, Ui};

use crate::app::{App, ClothingUploadStatus, Load};
use super::{accent, failed, h1, muted, primary_button, tile, waiting, wrap, Ctx, DANGER, GREEN, INK, MUTED};

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    h1(ui, "Clothing Upload");
    ui.add_space(4.0);
    ui.label(
        RichText::new("Scrape all clothing from a Roblox group and re-upload it to hexium.")
            .color(MUTED)
            .size(13.0),
    );
    ui.add_space(16.0);

    // Group ID row
    ui.horizontal(|ui| {
        let edit = egui::TextEdit::singleline(&mut app.clothing.group_id_input)
            .hint_text("Group ID")
            .desired_width(160.0);
        let response = ui.add(edit);
        let entered = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let busy = !app.clothing.items.is_idle();
        if (primary_button(ui, "Find Clothing").clicked() || entered) && !busy {
            app.clothing.items = Load::Idle;
            app.load_group_clothing();
        }
        if matches!(app.clothing.items, Load::Busy) {
            ui.add(egui::Spinner::new().size(14.0).color(accent()));
        }
    });

    ui.add_space(8.0);

    let loaded = match &app.clothing.items {
        Load::Idle => return,
        Load::Busy => return,
        Load::Err(error) => {
            failed(ui, error);
            return;
        }
        Load::Ready((items, icons)) => Some((items.clone(), icons.clone())),
    };

    if let Some((items, icons)) = loaded {
        show_items(app, cx, ui, items, icons);
    }
}

fn show_items(
    app: &mut App,
    cx: &Ctx,
    ui: &mut Ui,
    items: Vec<crate::api::models::CatalogItem>,
    icons: std::collections::HashMap<u64, String>,
) {
    if items.is_empty() {
        muted(ui, "No clothing found for that group.");
        return;
    }

    let total = items.len();
    let n_selected = items.iter().filter(|i| app.clothing.selected_ids.contains(&i.id)).count();

    // Header row: count + Select All / Deselect All
    ui.horizontal(|ui| {
        muted(ui, &format!("{total} items — {n_selected} selected"));
        ui.add_space(8.0);
        if ui.small_button("Select all").clicked() {
            for item in &items {
                app.clothing.selected_ids.insert(item.id);
            }
        }
        if ui.small_button("Deselect all").clicked() {
            app.clothing.selected_ids.clear();
        }
    });
    ui.add_space(8.0);

    // Price + upload controls
    ui.horizontal(|ui| {
        ui.label("Price (R$):");
        let edit = egui::TextEdit::singleline(&mut app.clothing.price_input)
            .hint_text("0")
            .desired_width(80.0);
        ui.add(edit);
        app.clothing.price_input.retain(|c: char| c.is_ascii_digit());

        ui.add_space(8.0);

        let all_settled = !app.clothing.upload_queue.is_empty()
            && app.clothing.upload_queue.iter().all(|i| {
                !matches!(i.status, ClothingUploadStatus::Pending | ClothingUploadStatus::Working)
            });

        if app.clothing.uploading {
            ui.add(egui::Spinner::new().size(14.0).color(accent()));
            muted(ui, "Uploading…");
        } else if all_settled {
            let ok = app.clothing.upload_queue.iter()
                .filter(|i| matches!(i.status, ClothingUploadStatus::Done(_))).count();
            let fail = app.clothing.upload_queue.iter()
                .filter(|i| matches!(i.status, ClothingUploadStatus::Failed(_))).count();
            ui.label(
                RichText::new(format!("{ok} uploaded, {fail} failed"))
                    .color(if fail > 0 { DANGER } else { GREEN })
                    .size(13.0),
            );
            if fail > 0 && primary_button(ui, "Retry Failed").clicked() {
                for item in app.clothing.upload_queue.iter_mut() {
                    if matches!(item.status, ClothingUploadStatus::Failed(_)) {
                        item.status = ClothingUploadStatus::Pending;
                    }
                }
                app.start_clothing_uploads();
            }
        } else if n_selected > 0 {
            if primary_button(ui, &format!("Upload {n_selected}")).clicked() {
                app.queue_all_clothing_uploads();
            }
        } else {
            muted(ui, "Select items to upload");
        }
    });

    ui.add_space(10.0);

    egui::ScrollArea::vertical()
        .max_height(600.0)
        .show(ui, |ui| {
            wrap(ui, |ui| {
                for item in &items {
                    let icon_url = icons.get(&item.id).cloned();
                    let status = app.clothing.upload_queue.iter()
                        .find(|i| i.source_id == item.id)
                        .map(|i| i.status.clone());
                    let uploading = app.clothing.uploading;

                    tile(ui, 240.0, |ui| {
                        ui.horizontal(|ui| {
                            // Checkbox (disabled while uploading)
                            let mut checked = app.clothing.selected_ids.contains(&item.id);
                            let cb = ui.add_enabled(
                                !uploading,
                                egui::Checkbox::new(&mut checked, ""),
                            );
                            if cb.changed() {
                                if checked {
                                    app.clothing.selected_ids.insert(item.id);
                                } else {
                                    app.clothing.selected_ids.remove(&item.id);
                                }
                            }

                            app.thumbs.show(ui, cx, icon_url.as_deref(), egui::vec2(42.0, 42.0));

                            ui.vertical(|ui| {
                                ui.label(RichText::new(&item.name).size(13.0).color(INK));
                                let type_label = match item.asset_type.as_str() {
                                    "TShirt" | "T-Shirt" => "T-Shirt",
                                    "Shirt" => "Shirt",
                                    "Pants" => "Pants",
                                    other => other,
                                };
                                ui.label(RichText::new(type_label).size(11.0).color(MUTED));
                                match &status {
                                    None => {}
                                    Some(ClothingUploadStatus::Pending) => {
                                        muted(ui, "Pending");
                                    }
                                    Some(ClothingUploadStatus::Working) => {
                                        ui.horizontal(|ui| {
                                            ui.add(egui::Spinner::new().size(10.0).color(accent()));
                                            ui.label(
                                                RichText::new("Uploading")
                                                    .size(11.0)
                                                    .color(accent()),
                                            );
                                        });
                                    }
                                    Some(ClothingUploadStatus::Done(new_id)) => {
                                        ui.label(
                                            RichText::new(format!("✓ ID {new_id}"))
                                                .size(11.0)
                                                .color(GREEN),
                                        );
                                    }
                                    Some(ClothingUploadStatus::Failed(error)) => {
                                        ui.label(
                                            RichText::new(format!("✗ {error}"))
                                                .size(11.0)
                                                .color(DANGER),
                                        );
                                    }
                                }
                            });
                        });
                    });
                }
            });
        });
}
