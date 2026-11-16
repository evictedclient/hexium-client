use eframe::egui::{self, RichText, Ui};

use crate::app::{App, Load, WriteAction};
use crate::ui::{self, accent, failed, h1, h2, muted, waiting, Ctx};

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    app.load_friends();
    app.load_friend_requests();

    h1(ui, "Friends");
    muted(ui, "People you know, and people who want to.");
    ui.add_space(4.0);

    h2(ui, "Friend requests");
    let mut ask: Option<WriteAction> = None;
    let mut open_profile: Option<u64> = None;
    match app.friends.requests.clone() {
        Load::Ready(bundle) => {
            if bundle.items.is_empty() {
                muted(ui, "No pending friend requests.");
            } else {
                ui::wrap(ui, |ui| {
                    for request in &bundle.items {
                        let icon = bundle.icons.get(&request.user_id).cloned();
                        ui::tile(ui, 300.0, |ui| {
                            ui.horizontal(|ui| {
                                app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(46.0, 46.0));
                                ui.vertical(|ui| {
                                    if ui
                                        .link(RichText::new(&request.display_name).size(12.5).strong())
                                        .clicked()
                                    {
                                        open_profile = Some(request.user_id);
                                    }
                                    muted(ui, &format!("@{}", request.name));
                                    if !request.created.is_empty() {
                                        muted(
                                            ui,
                                            &format!(
                                                "sent {}",
                                                request.created.replace('T', " ").chars().take(16).collect::<String>()
                                            ),
                                        );
                                    }
                                    ui.horizontal(|ui| {
                                        if ui.small_button("Accept").clicked() {
                                            ask = Some(WriteAction::AcceptFriend {
                                                user_id: request.user_id,
                                                name: request.name.clone(),
                                            });
                                        }
                                        if ui.small_button("Decline").clicked() {
                                            ask = Some(WriteAction::DeclineFriend {
                                                user_id: request.user_id,
                                                name: request.name.clone(),
                                            });
                                        }
                                    });
                                });
                            });
                        });
                    }
                });
            }
        }
        Load::Err(error) => {
            failed(ui, &format!("Could not load friend requests: {error}"));
            if ui.button("Retry").clicked() {
                app.friends.requests = Load::Idle;
                app.load_friend_requests();
            }
        }
        _ => waiting(ui, "friend requests"),
    }
    if let Some(action) = ask {
        app.ask(action);
    }
    if let Some(id) = open_profile {
        app.open_profile(id);
        return;
    }

    h2(ui, "Your friends");

    match app.friends.data.clone() {
        Load::Ready(bundle) => {
            let online = bundle.friends.iter().filter(|friend| friend.is_online).count();
            muted(
                ui,
                &format!("{} friends · {} online", bundle.friends.len(), online),
            );
            ui.add_space(6.0);
            ui::wrap(ui, |ui| {
                for friend in &bundle.friends {
                    let icon = bundle.icons.get(&friend.id).cloned();
                    let id = friend.id;
                    ui::tile(ui, 236.0, |ui| {
                        ui.horizontal(|ui| {
                            app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(46.0, 46.0));
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&friend.display_name).size(12.5).strong());
                                muted(ui, &format!("@{}", friend.name));
                                ui.horizontal(|ui| {
                                    ui::presence_dot(ui, friend.is_online);
                                    muted(
                                        ui,
                                        crate::api::models::presence_label(
                                            friend.presence,
                                            friend.is_online,
                                        ),
                                    );
                                    if ui.small_button("Profile").clicked() {
                                        app.open_profile(id);
                                    }
                                });
                                if let Some(place) = friend.place_id {
                                    if !friend.location.is_empty() {
                                        ui.label(
                                            RichText::new(&friend.location)
                                                .size(11.0)
                                                .color(accent()),
                                        );
                                    }
                                    let state = app
                                        .friends
                                        .join
                                        .as_ref()
                                        .filter(|(who, _)| *who == id)
                                        .map(|(_, state)| state.clone());
                                    let busy = matches!(app.friends.join, Some((_, Load::Busy)));
                                    let label = if matches!(state, Some(Load::Busy)) { "Joining…" } else { "Join friend" };
                                    if ui.add_enabled(!busy, egui::Button::new(label).small()).clicked() {
                                        app.join_friend(id, friend.name.clone(), place);
                                    }
                                    match state {
                                        Some(Load::Ready(year)) => muted(ui, &format!("Launching the {year} client…")),
                                        Some(Load::Err(error)) => {
                                            ui.label(RichText::new(error).size(11.0).color(ui::DANGER));
                                        }
                                        _ => {}
                                    }
                                }
                            });
                        });
                    });
                }
            });
        }
        Load::Err(error) => {
            failed(ui, &error);
            if ui.button("Retry").clicked() {
                app.friends.data = Load::Idle;
                app.load_friends();
            }
        }
        _ => waiting(ui, "friends"),
    }
}
