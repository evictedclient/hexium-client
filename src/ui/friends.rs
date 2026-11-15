use eframe::egui::{self, RichText, Ui};

use crate::app::{App, Load, WriteAction};
use crate::ui::{self, failed, h1, h2, muted, waiting, Ctx};

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
