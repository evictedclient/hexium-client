use eframe::egui::{self, RichText, Ui};

use crate::app::{App, Load};
use crate::ui::{self, failed, h1, h2, muted, waiting, Ctx, MUTED};

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    app.load_avatar();

    h1(ui, "Avatar");
    muted(ui, "What you are wearing right now.");
    ui.add_space(8.0);

    match app.avatar.data.clone() {
        Load::Ready(bundle) => {
            ui.horizontal_top(|ui| {
                app.thumbs.show(ui, cx, bundle.render.as_deref(), egui::vec2(280.0, 280.0));
                ui.add_space(12.0);

                // Each body part in its real colour, as a blocky R6 figure.
                let parts = body_parts(&bundle);
                ui.vertical(|ui| {
                    ui.add_space(8.0);
                    body_figure(ui, &parts);
                });
                ui.add_space(16.0);

                ui.vertical(|ui| {
                    ui.add_space(4.0);
                    ui::eyebrow(ui, "Body colours");
                    if parts.iter().all(|part| part.colour.is_none()) {
                        muted(ui, "No body colours returned.");
                    }
                    for part in &parts {
                        ui.horizontal(|ui| {
                            let (rect, _) =
                                ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                            ui.painter().rect_filled(
                                rect,
                                egui::Rounding::same(4.0),
                                part.colour.unwrap_or(ui::raised()),
                            );
                            ui.painter().rect_stroke(
                                rect,
                                egui::Rounding::same(4.0),
                                egui::Stroke::new(1.0, ui::line()),
                            );
                            ui.add_sized(
                                egui::vec2(72.0, 18.0),
                                egui::Label::new(RichText::new(part.label).font(ui::semibold(13.0))),
                            );
                            ui.label(RichText::new(&part.name).size(13.0));
                            if let Some(id) = part.id {
                                ui.label(RichText::new(format!("#{id}")).size(11.0).color(ui::FAINT));
                            }
                        });
                    }

                    ui.add_space(10.0);
                    ui::eyebrow(ui, "Scales");
                    ui.horizontal_wrapped(|ui| {
                        if !bundle.data.avatar_type.is_empty() {
                            ui::pill(ui, &bundle.data.avatar_type, ui::accent());
                        }
                        for (key, value) in &bundle.data.scales {
                            ui.label(
                                RichText::new(format!("{} {}", scale_label(key), as_percent(value)))
                                    .size(12.5)
                                    .color(MUTED),
                            );
                        }
                    });
                });
            });

            h2(ui, &format!("Worn assets ({})", bundle.data.assets.len()));

            if bundle.data.assets.is_empty() {
                muted(ui, "Nothing worn right now.");
            } else {
                let mut open_item: Option<(u64, String)> = None;
                ui::wrap(ui, |ui| {
                    for asset in &bundle.data.assets {
                        let icon = bundle.icons.get(&asset.id).cloned();
                        ui::tile(ui, 160.0, |ui| {
                            app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(136.0, 136.0));
                            if ui.link(RichText::new(&asset.name).size(11.5).strong()).clicked() {
                                open_item = Some((asset.id, asset.name.clone()));
                            }
                            ui.label(
                                RichText::new(if asset.asset_type.is_empty() {
                                    "asset"
                                } else {
                                    asset.asset_type.as_str()
                                })
                                .size(10.0)
                                .color(MUTED),
                            );
                        });
                    }
                });
                if let Some((asset, name)) = open_item {
                    app.open_item(asset, &name);
                }
            }

            if !bundle.data.scales.is_empty() || !bundle.data.body_colors.is_empty() {
                ui.add_space(10.0);
                muted(ui, "Changing your avatar happens on hexium.wtf for now.");
            }
        }
        Load::Err(error) => {
            failed(ui, &error);
            if ui.button("Retry").clicked() {
                app.avatar.data = Load::Idle;
                app.load_avatar();
            }
        }
        _ => waiting(ui, "avatar"),
    }
}

struct BodyPart {
    key: &'static str,
    label: &'static str,
    id: Option<u64>,
    name: String,
    colour: Option<egui::Color32>,
}

/// The six parts in a fixed order, each resolved against the site's palette.
fn body_parts(bundle: &crate::api::models::AvatarBundle) -> Vec<BodyPart> {
    const PARTS: [(&str, &str); 6] = [
        ("headColorId", "Head"),
        ("torsoColorId", "Torso"),
        ("leftArmColorId", "Left arm"),
        ("rightArmColorId", "Right arm"),
        ("leftLegColorId", "Left leg"),
        ("rightLegColorId", "Right leg"),
    ];
    PARTS
        .iter()
        .map(|&(key, label)| {
            let id = bundle
                .data
                .body_colors
                .iter()
                .find(|(name, _)| name == key)
                .and_then(|(_, value)| value.parse::<u64>().ok());
            let entry = id.and_then(|id| bundle.palette.get(&id));
            BodyPart {
                key,
                label,
                id,
                name: match (entry, id) {
                    (Some(entry), _) if !entry.name.is_empty() => entry.name.clone(),
                    (_, Some(id)) => format!("Colour {id}"),
                    _ => "Unknown".to_string(),
                },
                colour: entry.and_then(|entry| parse_hex(&entry.hex)),
            }
        })
        .collect()
}

/// A front-on R6 figure: head, torso, two arms, two legs, each in its colour.
fn body_figure(ui: &mut Ui, parts: &[BodyPart]) {
    let unit = 26.0;
    let gap = 3.0;
    let size = egui::vec2(unit * 4.0 + gap * 2.0, unit * 5.2 + gap * 2.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    let colour = |key: &str| {
        parts
            .iter()
            .find(|part| part.key == key)
            .and_then(|part| part.colour)
            .unwrap_or(ui::raised())
    };
    let left = rect.left();
    let top = rect.top();
    let head_side = unit * 1.2;
    let torso_left = left + unit + gap;
    let torso_top = top + head_side + gap;
    let blocks = [
        (
            "headColorId",
            egui::Rect::from_min_size(
                egui::pos2(left + size.x / 2.0 - head_side / 2.0, top),
                egui::vec2(head_side, head_side),
            ),
        ),
        (
            "torsoColorId",
            egui::Rect::from_min_size(egui::pos2(torso_left, torso_top), egui::vec2(unit * 2.0, unit * 2.0)),
        ),
        (
            "rightArmColorId",
            egui::Rect::from_min_size(egui::pos2(left, torso_top), egui::vec2(unit, unit * 2.0)),
        ),
        (
            "leftArmColorId",
            egui::Rect::from_min_size(
                egui::pos2(torso_left + unit * 2.0 + gap, torso_top),
                egui::vec2(unit, unit * 2.0),
            ),
        ),
        (
            "rightLegColorId",
            egui::Rect::from_min_size(
                egui::pos2(torso_left, torso_top + unit * 2.0 + gap),
                egui::vec2(unit - gap / 2.0, unit * 2.0),
            ),
        ),
        (
            "leftLegColorId",
            egui::Rect::from_min_size(
                egui::pos2(torso_left + unit + gap / 2.0, torso_top + unit * 2.0 + gap),
                egui::vec2(unit - gap / 2.0, unit * 2.0),
            ),
        ),
    ];
    for (key, block) in blocks {
        painter.rect_filled(block, egui::Rounding::same(4.0), colour(key));
        painter.rect_stroke(block, egui::Rounding::same(4.0), egui::Stroke::new(1.0, ui::line()));
    }
}

/// `#EAB892` -> colour.
fn parse_hex(hex: &str) -> Option<egui::Color32> {
    let digits = hex.trim().trim_start_matches('#');
    if digits.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(digits, 16).ok()?;
    Some(egui::Color32::from_rgb(
        (value >> 16) as u8,
        (value >> 8) as u8,
        value as u8,
    ))
}

fn scale_label(key: &str) -> &str {
    match key {
        "bodyType" => "Body type",
        "depth" => "Depth",
        "head" => "Head",
        "height" => "Height",
        "proportion" => "Proportion",
        "width" => "Width",
        other => other,
    }
}

/// Scales come back as fractions (1 = 100%).
fn as_percent(value: &str) -> String {
    match value.parse::<f64>() {
        Ok(number) => format!("{}%", (number * 100.0).round()),
        Err(_) => value.to_string(),
    }
}
