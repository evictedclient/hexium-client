use eframe::egui::{self, RichText, Ui};

use crate::app::{App, Load};
use crate::ui::{self, failed, h1, h2, muted, waiting, Ctx, MUTED};

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    app.load_groups();

    h1(ui, "Groups");
    muted(ui, "The groups you belong to. Open one for its roles and wall.");
    ui.add_space(8.0);

    match app.groups.data.clone() {
        Load::Ready(bundle) => {
            if bundle.groups.is_empty() {
                muted(ui, "You are not in any groups.");
            } else {
                ui::wrap(ui, |ui| {
                    for group in &bundle.groups {
                        let icon = bundle.icons.get(&group.id).cloned();
                        let id = group.id;
                        ui::tile(ui, 300.0, |ui| {
                            ui.horizontal(|ui| {
                                app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(52.0, 52.0));
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&group.name).size(13.0).strong());
                                    muted(
                                        ui,
                                        &format!(
                                            "role: {}",
                                            if group.role.is_empty() {
                                                "member"
                                            } else {
                                                group.role.as_str()
                                            }
                                        ),
                                    );
                                    muted(
                                        ui,
                                        &format!("{} members", ui::compact(group.member_count)),
                                    );
                                    if ui.small_button("Details").clicked() {
                                        app.groups.selected = Some(id);
                                        app.groups.detail = Load::Idle;
                                        app.load_group_detail(id);
                                    }
                                });
                            });
                        });
                    }
                });
            }
        }
        Load::Err(error) => {
            failed(ui, &error);
            if ui.button("Retry").clicked() {
                app.groups.data = Load::Idle;
                app.load_groups();
            }
        }
        _ => waiting(ui, "groups"),
    }
}

/// The selected group, in the docked panel.
pub fn dock(app: &mut App, cx: &Ctx, ctx: &egui::Context) {
    let detail = app.groups.detail.clone();
    if let Some(id) = app.groups.selected {
        let mut open = true;
        ui::dock(ctx, "group", 480.0, egui::Id::new(("group", id)), match &detail {
            Load::Ready(detail) if !detail.info.name.is_empty() => {
                format!("Group · {}", detail.info.name)
            }
            _ => format!("Group {id}"),
        }, &mut open, |ui| match detail {
                Load::Ready(detail) => {
                    ui.horizontal_top(|ui| {
                        app.thumbs.show(ui, cx, detail.icon.as_deref(), egui::vec2(72.0, 72.0));
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&detail.info.name).size(16.0).strong());
                            muted(
                                ui,
                                &format!(
                                    "{} members · owner {}",
                                    ui::compact(detail.info.member_count),
                                    if detail.info.owner.is_empty() {
                                        "unknown"
                                    } else {
                                        detail.info.owner.as_str()
                                    }
                                ),
                            );
                            if !detail.info.description.is_empty() {
                                ui.label(
                                    RichText::new(&detail.info.description)
                                        .size(11.5)
                                        .color(MUTED),
                                );
                            }
                        });
                    });

                    h2(ui, &format!("Roles ({})", detail.roles.len()));
                    for (name, count) in detail.roles.iter().take(20) {
                        muted(ui, &format!("{name} · {count} members"));
                    }

                    h2(ui, &format!("Wall ({})", detail.wall.len()));
                    if detail.wall.is_empty() {
                        muted(ui, "No wall posts.");
                    }
                    for post in detail.wall.iter().take(10) {
                        ui.horizontal_top(|ui| {
                            let icon = detail.wall_icons.get(&post.poster_id).cloned();
                            app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(24.0, 24.0));
                            ui.vertical(|ui| {
                                if post.poster_id != 0 {
                                    if ui.link(RichText::new(&post.poster).font(ui::semibold(11.5))).clicked() {
                                        app.open_profile(post.poster_id);
                                    }
                                } else {
                                    ui.label(RichText::new(&post.poster).font(ui::semibold(11.5)));
                                }
                                ui.label(RichText::new(&post.body).size(11.5));
                            });
                        });
                        ui.add_space(3.0);
                    }
                }
                Load::Err(error) => failed(ui, &error),
                _ => waiting(ui, "group details"),
            });
        if !open {
            app.groups.selected = None;
        }
    }
}
