use eframe::egui::{self, RichText, Ui};

use crate::api::endpoints::MESSAGE_TABS;
use crate::app::{App, Load, WriteAction};
use crate::ui::{self, failed, h1, muted, waiting, Ctx, accent, GOLD, MUTED};


pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    app.load_messages();

    h1(ui, "Messages");
    muted(ui, "Your inbox. Reply, archive or mark things read here; start a new message from someone's profile.");
    ui.add_space(8.0);

    let current = app.messages_tab();
    let mut picked: Option<&str> = None;
    ui.horizontal(|ui| {
        for (tab, label) in MESSAGE_TABS {
            if ui.selectable_label(current == tab, label).clicked() && current != tab {
                picked = Some(tab);
            }
        }
    });
    if let Some(tab) = picked {
        app.messages.tab = tab.to_string();
        app.reload_messages();
    }
    ui.add_space(4.0);

    match app.messages.data.clone() {
        Load::Ready(page) => {
            if page.items.is_empty() {
                muted(ui, "Nothing in this tab.");
                return;
            }

            let unread = page.items.iter().filter(|message| !message.is_read).count();
            let mut summary = format!("{} of {} loaded", page.items.len(), page.total);
            if unread > 0 {
                summary.push_str(&format!(" · {unread} unread"));
            }
            ui.horizontal(|ui| {
                muted(ui, &summary);
                if current == "inbox" {
                    let account_unread = app
                        .session
                        .ready()
                        .map(|session| session.counts.unread)
                        .unwrap_or(unread as u64);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if app.messages.preparing_all {
                            ui.add(egui::Spinner::new().size(13.0).color(accent()));
                            muted(ui, "Finding unread messages…");
                        } else if ui
                            .add_enabled(account_unread > 0, egui::Button::new("Mark all as read…"))
                            .clicked()
                        {
                            app.prepare_mark_all_read();
                        }
                    });
                }
            });
            ui.add_space(4.0);

            let sent = current == "sent";
            // Read state belongs to messages you received; announcements have none.
            let can_mark = current == "inbox" || current == "archive";
            let mut open_profile: Option<u64> = None;
            let mut ask: Option<WriteAction> = None;
            let mut reply: Option<crate::api::models::Message> = None;
            // Measured once: inside the loop each card would see the previous one's width
            // and the rows would creep wider.
            let width = (ui.available_width() - 16.0).max(320.0);
            for message in &page.items {
                ui::tile(ui, width, |ui| {
                    ui.horizontal(|ui| {
                        if !message.is_read {
                            ui::pill(ui, "UNREAD", accent());
                        }
                        if message.is_system {
                            ui::pill(ui, "SYSTEM", GOLD);
                        }
                        let subject = if message.subject.trim().is_empty() {
                            "(no subject)"
                        } else {
                            message.subject.as_str()
                        };
                        ui.label(RichText::new(subject).size(13.0).strong());
                        if can_mark {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let read = message.is_read;
                                let label = if read { "Mark unread" } else { "Mark read" };
                                if ui.small_button(label).clicked() {
                                    ask = Some(WriteAction::MarkRead {
                                        message_id: message.id,
                                        subject: subject.to_string(),
                                        read: !read,
                                    });
                                }
                                let archive = current == "inbox";
                                if ui
                                    .small_button(if archive { "Archive" } else { "Move to inbox" })
                                    .clicked()
                                {
                                    ask = Some(WriteAction::Archive {
                                        message_id: message.id,
                                        subject: subject.to_string(),
                                        archive,
                                    });
                                }
                                // System notices come from the site itself; there is no one to answer.
                                if !message.is_system && message.sender_id != 0 && ui.small_button("Reply").clicked() {
                                    reply = Some(message.clone());
                                }
                            });
                        }
                    });

                    ui.horizontal(|ui| {
                        let (prefix, name) = if sent {
                            ("to", message.recipient_name.as_str())
                        } else {
                            ("from", message.sender_name.as_str())
                        };
                        ui.label(RichText::new(prefix).size(11.0).color(MUTED));
                        let person = if sent { message.recipient_id } else { message.sender_id };
                        if let Some(icon) = page.icons.get(&person) {
                            app.thumbs.show(ui, cx, Some(icon.as_str()), egui::vec2(20.0, 20.0));
                        }
                        if sent || message.sender_id == 0 || name.is_empty() {
                            let shown = if name.is_empty() { "unknown" } else { name };
                            ui.label(RichText::new(shown).size(11.0).color(MUTED));
                        } else if ui.link(RichText::new(name).size(11.0)).clicked() {
                            open_profile = Some(message.sender_id);
                        }
                        ui.label(
                            RichText::new(format!(
                                "· {}",
                                message
                                    .created
                                    .replace('T', " ")
                                    .chars()
                                    .take(16)
                                    .collect::<String>()
                            ))
                            .size(11.0)
                            .color(MUTED),
                        );
                    });

                    if !message.body.trim().is_empty() {
                        ui.add_space(2.0);
                        ui.label(RichText::new(message.body.trim()).size(12.0));
                    }
                });
            }

            ui.add_space(6.0);
            if app.messages.loading_more {
                waiting(ui, "more messages");
            } else if (page.items.len() as u64) < page.total {
                if ui.button("Load more").clicked() {
                    app.load_more_messages();
                }
            } else {
                muted(ui, "End of the list.");
            }
            if let Some(error) = &app.messages.more_error {
                failed(ui, error);
            }

            if let Some(action) = ask {
                app.ask(action);
            }
            if let Some(message) = reply {
                app.compose_reply(&message);
            }
            if let Some(id) = open_profile {
                app.open_profile(id);
            }
        }
        Load::Err(error) => {
            failed(ui, &error);
            if ui.button("Retry").clicked() {
                app.reload_messages();
            }
        }
        _ => waiting(ui, "messages"),
    }
}

/// The message editor. Sending goes through the shared confirmation dialog.
pub fn compose_window(app: &mut App, ctx: &egui::Context) {
    let Some(draft) = app.compose.as_mut() else {
        return;
    };
    let title = if draft.reply_to.is_some() {
        format!("Reply to {}", draft.recipient)
    } else {
        format!("Message to {}", draft.recipient)
    };

    let mut open = true;
    let mut send: Option<WriteAction> = None;
    let mut problem: Option<&'static str> = None;
    egui::Window::new(title)
        .id(egui::Id::new("compose-window"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(460.0)
        .show(ctx, |ui| {
            ui.label(RichText::new("Subject").size(11.0).color(MUTED));
            ui.add(
                egui::TextEdit::singleline(&mut draft.subject)
                    .hint_text("Subject")
                    .char_limit(100)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(4.0);
            ui.label(RichText::new("Message").size(11.0).color(MUTED));
            ui.add(
                egui::TextEdit::multiline(&mut draft.body)
                    .hint_text("Write your message…")
                    .desired_rows(8)
                    .desired_width(f32::INFINITY),
            );
            if draft.reply_to.is_some() {
                ui.checkbox(&mut draft.include_previous, "Quote their message under mine");
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let ready = !draft.subject.trim().is_empty() && !draft.body.trim().is_empty();
                if ui.add_enabled(ready, egui::Button::new("Send…")).clicked() {
                    send = Some(WriteAction::SendMessage {
                        recipient_id: draft.recipient_id,
                        recipient: draft.recipient.clone(),
                        subject: draft.subject.trim().to_string(),
                        body: draft.body.trim().to_string(),
                        reply_to: draft.reply_to,
                        include_previous: draft.include_previous,
                    });
                }
                if !ready {
                    problem = Some("A subject and a message are both needed.");
                }
                muted(ui, &format!("{} characters", draft.body.chars().count()));
            });
            if let Some(problem) = problem {
                muted(ui, problem);
            }
        });

    if let Some(action) = send {
        app.ask(action);
    } else if !open {
        app.compose = None;
    }
}
