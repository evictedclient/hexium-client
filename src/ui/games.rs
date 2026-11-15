use eframe::egui::{self, RichText, Ui};

use crate::app::{App, Load};
use crate::ui::{self, failed, h1, muted, waiting, Ctx, MUTED};

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    app.load_sorts();

    h1(ui, "Games");
    muted(ui, "Find something to play.");
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(&mut app.games.keyword)
                .hint_text("Search games…")
                .desired_width(300.0),
        );
        let submitted =
            response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        if ui.button("Search").clicked() || submitted {
            app.reload_games();
        }
        if !app.games.keyword.is_empty() && ui.button("Clear").clicked() {
            app.games.keyword.clear();
            app.reload_games();
        }
    });

    ui.add_space(4.0);
    match app.games.sorts.clone() {
        Load::Ready(sorts) if !sorts.is_empty() => {
            ui::wrap(ui, |ui| {
                for (token, name) in sorts {
                    let selected = app.games.sort == token;
                    if ui.selectable_label(selected, name).clicked() && !selected {
                        app.games.sort = token;
                        app.reload_games();
                    }
                }
            });
        }
        Load::Err(error) => muted(
            ui,
            &format!("Sorts unavailable ({error}); the default sort is used."),
        ),
        _ => waiting(ui, "sorts"),
    }

    ui.add_space(8.0);
    match app.games.games.clone() {
        Load::Ready(bundle) => {
            if bundle.games.is_empty() {
                muted(ui, "No games here yet.");
            } else {
                ui::wrap(ui, |ui| {
                    for (index, game) in bundle.games.iter().enumerate() {
                        let icon = bundle
                            .icons
                            .get(&game.id)
                            .cloned()
                            .or_else(|| game.icon_url.clone());
                        let creator = if game.creator.is_empty() {
                            "unknown creator"
                        } else {
                            game.creator.as_str()
                        };
                        let selected = app.games.selected == Some(index);
                        let card = ui::tile_button(ui, 306.0, selected, |ui| {
                            ui.horizontal(|ui| {
                                app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(72.0, 72.0));
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&game.name).font(ui::semibold(14.0)));
                                    muted(ui, creator);
                                    muted(
                                        ui,
                                        &format!(
                                            "{} playing · {} visits",
                                            ui::compact(game.playing),
                                            ui::compact(game.visits)
                                        ),
                                    );
                                });
                            });
                        });
                        if card.on_hover_text("Open details").clicked() {
                            app.games.selected = Some(index);
                        }
                    }
                });
            }
        }
        Load::Err(error) => {
            failed(ui, &error);
            if ui.button("Retry").clicked() {
                app.reload_games();
            }
        }
        _ => waiting(ui, "games"),
    }

    let selected = app.games.selected.and_then(|index| {
        app.games.games.ready().and_then(|bundle| {
            bundle.games.get(index).map(|game| {
                (
                    game.clone(),
                    bundle
                        .icons
                        .get(&game.id)
                        .cloned()
                        .or_else(|| game.icon_url.clone()),
                )
            })
        })
    });

    if let Some((game, icon)) = selected {
        let mut open = true;
        egui::Window::new(format!("Game · {}", game.name))
            .id(egui::Id::new(("game", game.id)))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(470.0)
            .show(ui.ctx(), |ui| {
                ui.horizontal_top(|ui| {
                    app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(128.0, 128.0));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&game.name).size(17.0).strong());
                        let creator = if game.creator.is_empty() {
                            "unknown creator"
                        } else {
                            game.creator.as_str()
                        };
                        muted(ui, creator);
                        muted(ui, &format!("{} playing now", ui::compact(game.playing)));
                        muted(ui, &format!("{} visits", ui::compact(game.visits)));
                        muted(
                            ui,
                            &format!(
                                "{} up · {} down",
                                ui::compact(game.votes_up),
                                ui::compact(game.votes_down)
                            ),
                        );
                    });
                });
                ui.add_space(6.0);
                if !game.description.is_empty() {
                    ui.label(RichText::new(&game.description).size(12.0));
                    ui.add_space(6.0);
                }
                ui.horizontal(|ui| {
                    if ui.button("Open on hexium.wtf").clicked() {
                        cx.open(&format!("{}{}", cx.api.origin(), game.site_path()));
                    }
                    if let Some(place) = game.place_id {
                        ui.label(
                            RichText::new(format!("place {place} · universe {}", game.id))
                                .size(10.5)
                                .color(MUTED),
                        );
                    }
                });
            });
        if !open {
            app.games.selected = None;
        }
    }
}
