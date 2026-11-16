use eframe::egui::{self, RichText, Ui};

use crate::app::App;
use crate::ui::{self, h1, h2, muted, Ctx, DANGER, GREEN, MUTED};

pub fn show(app: &mut App, _cx: &Ctx, ui: &mut Ui) {
    h1(ui, "Settings");
    muted(
        ui,
        &format!(
            "These only change this app, not your account. hexium-client {}.",
            env!("CARGO_PKG_VERSION")
        ),
    );

    h2(ui, "Your session");
    ui::tile(ui, 620.0, |ui| {
        match app.cfg.masked_session() {
            Some(masked) => muted(ui, &masked),
            None => muted(ui, "No cookie stored."),
        }
        ui.horizontal(|ui| {
            muted(ui, "Endpoint");
            ui.label(RichText::new(app.api.origin()).monospace().size(11.0));
        });
        ui.horizontal(|ui| {
            muted(ui, "User agent");
            ui.label(RichText::new(crate::api::USER_AGENT).monospace().size(11.0));
        });
        ui.horizontal(|ui| {
            muted(ui, "Cookie file");
            ui.label(
                RichText::new(crate::config::config_path().display().to_string())
                    .size(11.0)
                    .color(MUTED),
            );
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("Test session").clicked() {
                app.probe();
            }
            match app.probe_result {
                Some(true) => ui::pill(ui, "SESSION OK", GREEN),
                Some(false) => ui::pill(ui, "REJECTED", DANGER),
                None => {}
            }
            if ui.button("Sign out").clicked() {
                app.sign_out();
            }
        });
    });

    h2(ui, "Make it yours");
    ui::card_frame().show(ui, |ui| {
        ui.set_width(600.0);
        ui::eyebrow(ui, "Accent");
        ui.horizontal(|ui| {
            for (key, label, color) in ui::ACCENTS {
                let selected = app.cfg.accent == key;
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(64.0, 54.0), egui::Sense::click());
                let response = response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(label);
                let painter = ui.painter();
                if selected || response.hovered() {
                    painter.rect_stroke(
                        rect,
                        egui::Rounding::same(10.0),
                        egui::Stroke::new(1.0, if selected { color } else { ui::line() }),
                    );
                }
                ui::hexagon(painter, rect.center_top() + egui::vec2(0.0, 20.0), 12.0, Some(color), None);
                if selected {
                    ui::hexagon(painter, rect.center_top() + egui::vec2(0.0, 20.0), 4.5, Some(ui::card()), None);
                }
                painter.text(
                    rect.center_bottom() - egui::vec2(0.0, 9.0),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(11.0),
                    if selected { ui::INK } else { ui::MUTED },
                );
                if response.clicked() && !selected {
                    app.cfg.accent = key.to_string();
                    app.cfg.save();
                }
            }
        });
        ui.add_space(8.0);
        ui::eyebrow(ui, "Background");
        ui.horizontal(|ui| {
            for (value, label) in [("midnight", "Midnight"), ("slate", "Slate")] {
                let selected = app.cfg.theme == value;
                if ui.selectable_label(selected, label).clicked() && !selected {
                    app.cfg.theme = value.to_string();
                    app.cfg.save();
                }
            }
        });
    });

    h2(ui, "Chat");
    ui::tile(ui, 620.0, |ui| {
        if ui
            .checkbox(&mut app.cfg.share_typing, "Show people when I am typing")
            .changed()
        {
            app.cfg.save();
            if !app.cfg.share_typing {
                app.stop_typing();
            }
        }
        muted(ui, "While on, the other person sees a typing indicator as you write, the same as on the site.");
        ui.add_space(6.0);
        if ui
            .checkbox(&mut app.cfg.chat_notifications, "Desktop notifications for new chat messages")
            .changed()
        {
            app.cfg.save();
        }
        muted(ui, "Shown when a message arrives while the client is in the background or you are on another screen or chat. Needs the live connection.");
        ui.add_space(6.0);
        if ui
            .checkbox(&mut app.cfg.close_to_tray, "Close to tray")
            .changed()
        {
            app.cfg.save();
        }
        muted(ui, "Closing the window keeps the client running in the notification area, so chat keeps arriving. Quit from the tray icon's menu.");
        ui.add_space(6.0);
        if ui
            .checkbox(&mut app.cfg.activity_notifications, "Notifications for trades, friend requests and messages")
            .changed()
        {
            app.cfg.save();
        }
        muted(ui, "Checked once a minute, since the site has no live push for these.");
        ui.add_space(6.0);
        let mut at_sign_in = app.autostart != crate::autostart::State::Off;
        if ui.checkbox(&mut at_sign_in, "Open at sign-in").changed() {
            app.set_autostart(at_sign_in);
        }
        muted(ui, "Starts the client hidden in the tray when you log into Windows, so notifications arrive without opening it.");
        if app.autostart == crate::autostart::State::DisabledInTaskManager {
            ui.label(
                RichText::new("Switched off in Task Manager > Startup apps. Turn it back on there for it to run.")
                    .size(12.0)
                    .color(ui::GOLD),
            );
        }
    });


    h2(ui, "What this client talks to");
    ui::tile(ui, 620.0, |ui| {
        for line in [
            "users/v1 — authenticated user, profiles, status, username history",
            "economy/v1 + economy/v2 — Robux and Tickets balances, transaction history",
            "friends/v1 + presence/v1 — friend list, counts, online state",
            "thumbnails/v1 — batched headshots, avatars, game and group icons",
            "games/v1 + games/v2 — sorts, game list, a user's games",
            "catalog/v1 — search, navigation menu, item details (POST)",
            "avatar/v1 — worn assets, scales, body colours",
            "inventory/v1 — owned asset ids and collectibles",
            "groups/v1 + groups/v2 — groups, roles, wall posts",
            "privatemessages/v1 — inbox, sent, archive and announcements",
            "trades/v1 — inbound, outbound, completed and inactive trades, trade details",
            "chat/v2 + the /chat SignalR hub — conversations, messages, live updates",
            "api/alerts, auth/maintenance — banners",
        ] {
            muted(ui, line);
        }
    });

    ui.add_space(10.0);
    muted(
        ui,
        "Not wired yet: purchases, Robux in trades, chat, and avatar edits. Those are write \
         paths the capture never exercised, so this client stops at reading.",
    );
    muted(
        ui,
        "“Open on hexium.wtf” hands the page to your browser rather than guessing a launch URI.",
    );
    muted(
        ui,
        "Writes so far: follow and unfollow, friend requests (send, accept, decline) and unfriending, favourites, your status and description, game votes, message read state and archiving, sending private messages and replies, chat messages, accepting, declining or countering inbound trades, sending new trades, and listing collectibles for sale. Each one asks for confirmation before anything is sent.",
    );
}
