use eframe::egui::{self, RichText, Ui};

use crate::app::{App, Load, WriteAction};
use crate::ui::{self, failed, h1, h2, muted, waiting, Ctx, accent, DANGER, GREEN, MUTED};

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    app.load_profile();

    match app.profile.data.clone() {
        Load::Ready(bundle) => {
            let user = &bundle.user;
            ui.horizontal_top(|ui| {
                app.thumbs.show(ui, cx, bundle.avatar.as_deref(), egui::vec2(150.0, 150.0));
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(&user.display_name).font(ui::display(30.0)));
                    ui.horizontal(|ui| {
                        muted(ui, &format!("@{}", user.name));
                        if user.has_admin_badge {
                            ui::pill(ui, "ADMIN", accent());
                        }
                        if user.has_verified_badge {
                            ui::pill(ui, "VERIFIED", GREEN);
                        }
                        if user.is_banned {
                            ui::pill(ui, "BANNED", DANGER);
                        }
                    });
                    if !bundle.status.is_empty() {
                        ui.label(RichText::new(&bundle.status).size(12.5).italics());
                    }
                    if !user.description.is_empty() {
                        ui.add_space(2.0);
                        ui.label(RichText::new(&user.description).size(12.0).color(MUTED));
                    }
                    ui.add_space(4.0);
                    muted(ui, &format!("Joined {}", ui::first_ten(&user.created)));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{} friends", bundle.friends.len())).size(12.0));
                        ui.label(
                            RichText::new(format!("{} followers", ui::compact(bundle.followers)))
                                .size(12.0),
                        );
                        ui.label(
                            RichText::new(format!("{} following", ui::compact(bundle.following)))
                                .size(12.0),
                        );
                        ui.label(
                            RichText::new(format!("{} posts", ui::compact(user.post_count)))
                                .size(12.0),
                        );
                    });
                    let mine = app.session.ready().map(|session| session.user.id) == Some(user.id);
                    ui.horizontal(|ui| {
                        if ui.button("Open on hexium.wtf").clicked() {
                            cx.open(&format!(
                                "{}/users/{}/profile",
                                cx.api.origin(),
                                user.id
                            ));
                        }
                        if !mine {
                            match app.profile.following.clone() {
                                Load::Ready(following) => {
                                    let label = if following { "Unfollow" } else { "Follow" };
                                    if ui.button(label).clicked() {
                                        app.ask(ui::follow_action(user.id, &user.name, !following));
                                    }
                                }
                                Load::Err(_) => muted(ui, "follow state unavailable"),
                                _ => {
                                    ui.add(egui::Spinner::new().size(12.0));
                                }
                            }
                            let id = user.id;
                            let name = user.name.clone();
                            match app.profile.friendship.clone() {
                                Load::Ready(status) => match status.as_str() {
                                    "Friends" => {
                                        if ui.button("Unfriend").clicked() {
                                            app.ask(WriteAction::Unfriend { user_id: id, name });
                                        }
                                    }
                                    "RequestSent" => {
                                        ui.add_enabled(false, egui::Button::new("Request sent"));
                                    }
                                    "RequestReceived" => {
                                        if ui.button("Accept request").clicked() {
                                            app.ask(WriteAction::AcceptFriend { user_id: id, name: name.clone() });
                                        }
                                        if ui.button("Decline").clicked() {
                                            app.ask(WriteAction::DeclineFriend { user_id: id, name });
                                        }
                                    }
                                    "NotFriends" => {
                                        if ui.button("Add friend").clicked() {
                                            app.ask(WriteAction::RequestFriend { user_id: id, name });
                                        }
                                    }
                                    other => muted(ui, &format!("friendship: {other}")),
                                },
                                Load::Err(_) => muted(ui, "friend state unavailable"),
                                _ => {
                                    ui.add(egui::Spinner::new().size(12.0));
                                }
                            }
                            if ui.button("Message").clicked() {
                                app.compose_to(user.id, &user.name);
                            }
                        }
                    });

                    if mine {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut app.profile.status_input)
                                    .hint_text("Set a status…")
                                    .char_limit(254)
                                    .desired_width(260.0),
                            );
                            let changed = app.profile.status_input.trim() != bundle.status.trim();
                            if ui
                                .add_enabled(changed, egui::Button::new("Save status"))
                                .clicked()
                            {
                                app.ask(WriteAction::SetStatus {
                                    status: app.profile.status_input.trim().to_string(),
                                });
                            }
                        });
                    }
                });
            });

            if !bundle.badges.is_empty() {
                h2(ui, "Badges");
                ui::wrap(ui, |ui| {
                    for badge in &bundle.badges {
                        ui::tile(ui, 214.0, |ui| {
                            ui.label(RichText::new(&badge.name).size(12.0).strong());
                            if !badge.description.is_empty() {
                                muted(ui, &badge.description);
                            }
                        });
                    }
                });
            }

            h2(ui, &format!("Games ({})", bundle.games.len()));
            if bundle.games.is_empty() {
                muted(ui, "No published games.");
            } else {
                ui::wrap(ui, |ui| {
                    for game in &bundle.games {
                        let icon = bundle
                            .game_icons
                            .get(&game.id)
                            .cloned()
                            .or_else(|| game.icon_url.clone());
                        ui::tile(ui, 282.0, |ui| {
                            ui.horizontal(|ui| {
                                app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(56.0, 56.0));
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&game.name).size(12.5).strong());
                                    muted(
                                        ui,
                                        &format!(
                                            "{} visits · {} playing",
                                            ui::compact(game.visits),
                                            ui::compact(game.playing)
                                        ),
                                    );
                                });
                            });
                        });
                    }
                });
            }

            h2(ui, &format!("Groups ({})", bundle.groups.len()));
            if bundle.groups.is_empty() {
                muted(ui, "Not in any groups.");
            } else {
                ui::wrap(ui, |ui| {
                    for group in &bundle.groups {
                        let icon = bundle.group_icons.get(&group.id).cloned();
                        ui::tile(ui, 250.0, |ui| {
                            ui.horizontal(|ui| {
                                app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(44.0, 44.0));
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&group.name).size(12.5).strong());
                                    muted(
                                        ui,
                                        if group.role.is_empty() { "member" } else { &group.role },
                                    );
                                    muted(
                                        ui,
                                        &format!("{} members", ui::compact(group.member_count)),
                                    );
                                });
                            });
                        });
                    }
                });
            }

            h2(ui, &format!("Friends ({})", bundle.friends.len()));
            if bundle.friends.is_empty() {
                muted(ui, "No friends returned.");
            } else {
                ui::wrap(ui, |ui| {
                    for friend in &bundle.friends {
                        let icon = bundle.icons.get(&friend.id).cloned();
                        let id = friend.id;
                        ui::tile(ui, 206.0, |ui| {
                            ui.horizontal(|ui| {
                                app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(34.0, 34.0));
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&friend.display_name).size(12.0));
                                    ui.horizontal(|ui| {
                                        ui::presence_dot(ui, friend.is_online);
                                        if ui.small_button("Open").clicked() {
                                            app.open_profile(id);
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
            h1(ui, "Profile");
            failed(ui, &error);
            if ui.button("Retry").clicked() {
                app.profile.data = Load::Idle;
                app.load_profile();
            }
        }
        _ => {
            h1(ui, "Profile");
            waiting(ui, "profile, badges, groups and games");
        }
    }
}
