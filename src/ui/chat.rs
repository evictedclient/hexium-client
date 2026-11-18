//! Chat: your conversations on the left, the open thread on the right. New
//! messages arrive over the site's SignalR hub (with polling as a fallback), and
//! sending goes through the confirmation dialog like every other write.

use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{self, RichText, Sense, Stroke, Ui};

use crate::api::hub::HubStatus;
use crate::api::models::{ChatConversation, ChatMessage};
use crate::app::{App, ChatTarget, Load, WriteAction};
use crate::ui::{self, accent, failed, h1, muted, waiting, Ctx, FAINT, GREEN, INK, MUTED};

/// Longest message the composer takes; the site's own box has the same cap.
const MAX_LENGTH: usize = 160;

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    app.load_chat();

    ui.horizontal(|ui| {
        h1(ui, "Chat");
        ui.add_space(10.0);
        hub_badge(ui, app.chat.hub.as_ref());
    });
    muted(ui, "Conversations with your friends. New messages show up as they arrive.");
    ui.add_space(10.0);

    let bundle = match app.chat.list.clone() {
        Load::Ready(bundle) => bundle,
        Load::Err(error) => {
            failed(ui, &format!("Could not load your chats: {error}"));
            return;
        }
        _ => {
            waiting(ui, "chats");
            return;
        }
    };
    if !bundle.chat_enabled {
        ui::card_frame().show(ui, |ui| {
            muted(ui, "The site has chat switched off for this account, so nothing can be sent.");
        });
        ui.add_space(8.0);
    }

    let me = app.session.ready().map(|session| session.user.id).unwrap_or(0);
    let height = (ui.ctx().screen_rect().height() - 230.0).max(360.0);
    let list_width = 270.0;

    let mut open: Option<ChatTarget> = None;
    ui.horizontal_top(|ui| {
        // -- the list --------------------------------------------------------
        ui.allocate_ui_with_layout(
            egui::vec2(list_width, height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(list_width);
                let response = ui.add(
                    egui::TextEdit::singleline(&mut app.chat.filter)
                        .hint_text("Search people…")
                        .desired_width(list_width),
                );
                if std::mem::take(&mut app.focus_search) {
                    response.request_focus();
                }
                ui.add_space(6.0);
                let needle = app.chat.filter.trim().to_lowercase();
                egui::ScrollArea::vertical()
                    .id_salt("chat-list")
                    .max_height(height - 36.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut shown = 0;
                        for conversation in &bundle.conversations {
                            let label = conversation.label(me);
                            if !needle.is_empty() && !label.to_lowercase().contains(&needle) {
                                continue;
                            }
                            shown += 1;
                            let selected = app.chat.target == Some(ChatTarget::Conversation(conversation.id));
                            let typing = typing_in(app, conversation.id).is_some();
                            let icon = conversation
                                .other(me)
                                .and_then(|other| bundle.icons.get(&other.id))
                                .cloned();
                            if conversation_row(ui, &mut app.thumbs, cx, conversation, &label, icon.as_deref(), me, selected, typing) {
                                open = Some(ChatTarget::Conversation(conversation.id));
                            }
                        }
                        if bundle.conversations.is_empty() {
                            muted(ui, "No conversations yet.");
                        }

                        let friends: Vec<_> = bundle
                            .friends
                            .iter()
                            .filter(|friend| {
                                needle.is_empty()
                                    || friend.name.to_lowercase().contains(&needle)
                                    || friend.display_name.to_lowercase().contains(&needle)
                            })
                            .collect();
                        if !friends.is_empty() {
                            ui.add_space(10.0);
                            ui::eyebrow(ui, "Start a chat");
                            for friend in friends {
                                shown += 1;
                                let target = ChatTarget::NewWith {
                                    user_id: friend.id,
                                    name: friend.name.clone(),
                                };
                                let selected = app.chat.target.as_ref() == Some(&target);
                                let icon = bundle.icons.get(&friend.id).cloned();
                                let subtitle = if friend.is_online { "Online" } else { "Say hi" };
                                if person_row(ui, &mut app.thumbs, cx, &friend.name, subtitle, icon.as_deref(), selected, friend.is_online) {
                                    open = Some(target);
                                }
                            }
                        }
                        if shown == 0 && !needle.is_empty() {
                            muted(ui, "Nobody matches.");
                        }
                    });
            },
        );

        ui.add_space(12.0);

        // -- the thread ------------------------------------------------------
        let thread_width = (ui.available_width()).max(320.0);
        ui.allocate_ui_with_layout(
            egui::vec2(thread_width, height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(thread_width);
                thread(app, ui, &bundle.conversations, me, height, bundle.chat_enabled);
            },
        );
    });

    if let Some(target) = open {
        app.open_chat(target);
    }
}

fn hub_badge(ui: &mut Ui, status: Option<&HubStatus>) {
    let (text, color, hover) = match status {
        Some(HubStatus::Live) => ("LIVE", GREEN, "Connected to the site's chat hub.".to_string()),
        Some(HubStatus::Connecting) | None => ("CONNECTING", MUTED, "Connecting to the chat hub…".to_string()),
        Some(HubStatus::Offline(reason)) => (
            "POLLING",
            ui::GOLD,
            format!("The chat hub is not connected ({reason}). Checking for new messages every few seconds instead."),
        ),
    };
    ui.vertical(|ui| {
        ui.add_space(10.0);
        ui::pill(ui, text, color);
    })
    .response
    .on_hover_text(hover);
}

#[allow(clippy::too_many_arguments)]
fn conversation_row(
    ui: &mut Ui,
    thumbs: &mut ui::Thumbs,
    cx: &Ctx,
    conversation: &ChatConversation,
    label: &str,
    icon: Option<&str>,
    me: u64,
    selected: bool,
    typing: bool,
) -> bool {
    let preview = if typing {
        "typing…".to_string()
    } else {
        match &conversation.latest {
            Some(latest) if !latest.content.is_empty() => {
                let prefix = if latest.sender_id == me { "You: " } else { "" };
                let text: String = latest.content.chars().take(40).collect();
                format!("{prefix}{text}")
            }
            _ => "No messages yet".to_string(),
        }
    };
    let when = conversation
        .latest
        .as_ref()
        .map(|latest| short_when(&latest.sent))
        .unwrap_or_default();
    row(ui, selected, |ui| {
        thumbs.show(ui, cx, icon, egui::vec2(38.0, 38.0));
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(label)
                        .font(ui::semibold(13.0))
                        .color(INK),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if conversation.unread {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), Sense::hover());
                        ui.painter().circle_filled(rect.center(), 4.0, accent());
                    }
                    ui.label(RichText::new(when).size(10.5).color(FAINT));
                });
            });
            let color = if typing {
                accent()
            } else if conversation.unread {
                INK
            } else {
                MUTED
            };
            ui.label(RichText::new(preview).size(11.5).color(color));
        });
    })
}

#[allow(clippy::too_many_arguments)]
fn person_row(
    ui: &mut Ui,
    thumbs: &mut ui::Thumbs,
    cx: &Ctx,
    name: &str,
    subtitle: &str,
    icon: Option<&str>,
    selected: bool,
    online: bool,
) -> bool {
    row(ui, selected, |ui| {
        thumbs.show(ui, cx, icon, egui::vec2(30.0, 30.0));
        ui.vertical(|ui| {
            ui.label(RichText::new(name).font(ui::semibold(12.5)).color(INK));
            ui.label(
                RichText::new(subtitle)
                    .size(11.0)
                    .color(if online { GREEN } else { MUTED }),
            );
        });
    })
}

/// A clickable full-width row; returns true when clicked.
fn row(ui: &mut Ui, selected: bool, add: impl FnOnce(&mut Ui)) -> bool {
    let shown = egui::Frame::none()
        .fill(if selected { ui::tint(accent(), 40) } else { ui::card() })
        .stroke(Stroke::new(1.0, if selected { accent() } else { ui::line() }))
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(add);
        });
    ui.add_space(3.0);
    let response = shown
        .response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() && !selected {
        ui.painter().rect_stroke(
            response.rect,
            egui::Rounding::same(8.0),
            Stroke::new(1.0, ui::tint(accent(), 160)),
        );
    }
    response.clicked()
}

fn thread(
    app: &mut App,
    ui: &mut Ui,
    conversations: &[ChatConversation],
    me: u64,
    height: f32,
    enabled: bool,
) {
    let Some(target) = app.chat.target.clone() else {
        ui::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(height - 24.0);
            ui.vertical_centered(|ui| {
                ui.add_space(height / 3.0);
                ui.label(RichText::new("Pick a conversation").font(ui::display(20.0)).color(INK));
                muted(ui, "or a friend under \"Start a chat\".");
            });
        });
        return;
    };

    // Header: who, and a link to their profile.
    let (title, other_id, conversation_id) = match &target {
        ChatTarget::Conversation(id) => {
            let conversation = conversations.iter().find(|item| item.id == *id);
            (
                conversation.map(|item| item.label(me)).unwrap_or_else(|| format!("Conversation {id}")),
                conversation.and_then(|item| item.other(me)).map(|other| other.id),
                Some(*id),
            )
        }
        ChatTarget::NewWith { user_id, name } => (name.clone(), Some(*user_id), None),
    };
    let names = participant_names(conversations, conversation_id);

    let mut open_profile: Option<u64> = None;
    let mut ask: Option<WriteAction> = None;
    ui::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(&title).font(ui::display(19.0)).color(INK));
            if let Some(id) = other_id {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Profile").clicked() {
                        open_profile = Some(id);
                    }
                });
            }
        });
        ui.separator();

        let thread_height = height - 140.0;
        match app.chat.messages.clone() {
            Load::Ready(messages) => {
                let arrivals = app.chat.arrivals;
                let scroll = egui::ScrollArea::vertical()
                    .id_salt(("chat-thread", conversation_id))
                    .max_height(thread_height)
                    .min_scrolled_height(thread_height)
                    .auto_shrink([false, false])
                    .stick_to_bottom(true);
                scroll.show(ui, |ui| {
                    if messages.is_empty() {
                        ui.add_space(12.0);
                        muted(
                            ui,
                            if conversation_id.is_none() {
                                "No messages yet. The conversation starts with your first message."
                            } else {
                                "No messages yet."
                            },
                        );
                    }
                    let width = ui.available_width();
                    let mut last_day = String::new();
                    for message in &messages {
                        let day: String = message.sent.chars().take(10).collect();
                        if !day.is_empty() && day != last_day {
                            ui.add_space(6.0);
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new(&day).size(10.5).color(FAINT));
                            });
                            last_day = day;
                        }
                        if let Some(id) = bubble(ui, message, me, width, &names) {
                            open_profile = Some(id);
                        }
                    }
                    // Scroll to the newest once per arrival.
                    let seen_key = egui::Id::new(("chat-seen", conversation_id));
                    let seen = ui.data(|data| data.get_temp::<u64>(seen_key));
                    if seen != Some(arrivals) {
                        ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                        ui.data_mut(|data| data.insert_temp(seen_key, arrivals));
                    }
                });
            }
            Load::Err(error) => {
                ui.set_min_height(thread_height);
                failed(ui, &format!("Could not load messages: {error}"));
            }
            _ => {
                ui.set_min_height(thread_height);
                waiting(ui, "messages");
            }
        }

        // Typing line (kept the same height so the composer does not jump).
        let typing = conversation_id.and_then(|id| typing_in(app, id));
        let typing_text = match typing {
            Some(user) => format!("{} is typing…", names.get(&user).cloned().unwrap_or_else(|| "Someone".to_string())),
            None => String::new(),
        };
        ui.label(RichText::new(typing_text).size(11.0).color(accent()));

        // Composer.
        let recipient_id = other_id.unwrap_or(0);
        ui.horizontal(|ui| {
            let count = app.chat.draft.chars().count();
            let editor = egui::TextEdit::singleline(&mut app.chat.draft)
                .hint_text(if enabled { "Write a message…" } else { "Chat is switched off" })
                .char_limit(MAX_LENGTH)
                .desired_width(ui.available_width() - 120.0);
            let response = ui.add_enabled(enabled, editor);
            if response.changed() && !app.chat.draft.is_empty() {
                app.note_typing();
            }
            let text = app.chat.draft.trim().to_string();
            let ready = enabled && !text.is_empty() && (conversation_id.is_some() || recipient_id != 0);
            let enter = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let button = egui::Button::new(RichText::new("Send…").font(ui::semibold(13.0)).color(egui::Color32::from_rgb(18, 18, 20)))
                .fill(if ready { accent() } else { ui::raised() });
            let clicked = ui.add_enabled(ready, button).clicked();
            ui.label(RichText::new(format!("{count}/{MAX_LENGTH}")).size(10.5).color(FAINT));
            if ready && (clicked || enter) {
                ask = Some(WriteAction::SendChat {
                    conversation_id,
                    user_id: recipient_id,
                    recipient: title.clone(),
                    text,
                });
            }
        });
    });

    if let Some(action) = ask {
        // Pressing send ends the typing broadcast, as Enter does on the site.
        app.stop_typing();
        app.ask(action);
    }
    if let Some(id) = open_profile {
        app.open_profile(id);
    }
}

/// One message: yours on the right in the accent colour, theirs on the left.
/// Draws one message; returns the sender's id when their name is clicked.
fn bubble(ui: &mut Ui, message: &ChatMessage, me: u64, width: f32, names: &std::collections::HashMap<u64, String>) -> Option<u64> {
    let mut clicked = None;
    let mine = message.sender_id == me;
    let max = (width * 0.7).max(160.0);
    let layout = if mine {
        egui::Layout::right_to_left(egui::Align::Min)
    } else {
        egui::Layout::left_to_right(egui::Align::Min)
    };
    ui.allocate_ui_with_layout(egui::vec2(width, 0.0), layout, |ui| {
        ui.vertical(|ui| {
            egui::Frame::none()
                .fill(if mine { ui::tint(accent(), 70) } else { ui::raised() })
                .rounding(egui::Rounding {
                    nw: 12.0,
                    ne: 12.0,
                    sw: if mine { 12.0 } else { 3.0 },
                    se: if mine { 3.0 } else { 12.0 },
                })
                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                .show(ui, |ui| {
                    ui.set_max_width(max);
                    if !mine && names.len() > 1 {
                        if let Some(name) = names.get(&message.sender_id) {
                            if ui.link(RichText::new(name).font(ui::semibold(10.5)).color(accent())).clicked() {
                                clicked = Some(message.sender_id);
                            }
                        }
                    }
                    ui.add(egui::Label::new(RichText::new(&message.content).size(13.0).color(INK)).wrap());
                });
            let time: String = message.sent.chars().skip(11).take(5).collect();
            if !time.is_empty() {
                ui.label(RichText::new(time).size(9.5).color(FAINT));
            }
        });
    });
    ui.add_space(2.0);
    clicked
}

/// Names of everyone in a conversation, by id.
fn participant_names(
    conversations: &[ChatConversation],
    id: Option<u64>,
) -> std::collections::HashMap<u64, String> {
    id.and_then(|id| conversations.iter().find(|item| item.id == id))
        .map(|conversation| {
            conversation
                .participants
                .iter()
                .map(|participant| (participant.id, participant.name.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// Who is typing in a conversation right now, if anyone.
fn typing_in(app: &App, conversation: u64) -> Option<u64> {
    let (user, until) = app.chat.typing.get(&conversation)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0);
    (now < *until).then_some(*user)
}

/// `2026-11-16T18:04:00Z` -> `18:04` today, else `11-16`.
fn short_when(sent: &str) -> String {
    if sent.len() < 16 {
        return String::new();
    }
    let today = today_utc();
    if sent.starts_with(&today) {
        sent.chars().skip(11).take(5).collect()
    } else {
        sent.chars().skip(5).take(5).collect()
    }
}

fn today_utc() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_when_formats() {
        assert_eq!(short_when("2001-02-03T04:05:06Z"), "02-03");
        assert_eq!(short_when(""), "");
        assert_eq!(today_utc().len(), 10);
    }
}
