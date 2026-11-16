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
        "Writes so far: follow and unfollow, friend requests (send, accept, decline) and unfriending, favourites, your status and description, game votes, message read state and archiving, sending private messages and replies, accepting, declining or countering inbound trades, sending new trades, and listing collectibles for sale. Each one asks for confirmation before anything is sent.",
    );
}
