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

    h2(ui, "Mass send");
    ui::tile(ui, 620.0, |ui| {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Send to at most");
            changed |= ui
                .add(egui::DragValue::new(&mut app.cfg.mass_cap).range(1..=usize::MAX).speed(0.5).suffix(" people"))
                .changed();
        });
        ui.horizontal(|ui| {
            ui.label("Wait between trades");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut app.cfg.mass_gap_secs)
                        .range(crate::app::MASS_MIN_GAP..=u64::MAX)
                        .speed(0.5)
                        .suffix(" s"),
                )
                .on_hover_text(format!(
                    "At least {} s: Hexium refuses trades sent back to back.",
                    crate::app::MASS_MIN_GAP
                ))
                .changed();
        });
        if ui.small_button("Back to 10 people, 30 s").clicked() {
            app.cfg.mass_cap = 10;
            app.cfg.mass_gap_secs = 30;
            changed = true;
        }
        if changed {
            app.cfg.save();
        }
        muted(
            ui,
            "Used by Trade helpers. The wait has a 10-second floor: bursts of identical trades are what get accounts rate-limited or flagged.",
        );
    });

    h2(ui, "Display");
    ui::tile(ui, 620.0, |ui| {
        ui.horizontal(|ui| {
            ui.label("Text size");
            let mut percent = (app.cfg.ui_scale * 100.0).round();
            if ui.add(egui::Slider::new(&mut percent, 80.0..=150.0).suffix("%").step_by(5.0)).changed() {
                app.cfg.ui_scale = percent / 100.0;
                app.cfg.save();
            }
            if ui.small_button("Reset").clicked() {
                app.cfg.ui_scale = 1.0;
                app.cfg.save();
            }
        });
        if ui.checkbox(&mut app.cfg.compact, "Compact layout (tighter spacing)").changed() {
            app.cfg.save();
        }
        ui.add_space(6.0);
        ui::eyebrow(ui, "Keyboard shortcuts");
        for (keys, what) in [
            ("Ctrl+1 … Ctrl+9", "the side menu screens, top to bottom"),
            ("Ctrl+F", "the current screen's search box"),
            ("Ctrl+R or F5", "refresh this screen"),
            ("Esc", "close the side panel"),
            ("Ctrl + / Ctrl -", "zoom in and out (until the next restart)"),
        ] {
            ui.horizontal(|ui| {
                ui.label(RichText::new(keys).font(ui::semibold(12.0)));
                muted(ui, what);
            });
        }
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
        muted(ui, "Checked once a minute, since the site has no live push for these. The same switch covers Heximons value changes to your items.");
        ui.add_space(6.0);
        let mut changed = ui.checkbox(&mut app.cfg.deal_alerts, "Deal notifications").changed();
        ui.add_enabled_ui(app.cfg.deal_alerts, |ui| {
            ui.horizontal(|ui| {
                ui.label("At least");
                changed |= ui
                    .add(egui::Slider::new(&mut app.cfg.deal_alert_min, 10.0..=80.0).suffix("% below value").step_by(5.0))
                    .changed();
            });
            ui.horizontal(|ui| {
                changed |= ui.checkbox(&mut app.cfg.deal_alert_staff_only, "Staff-valued only").changed();
                changed |= ui.checkbox(&mut app.cfg.deal_alert_high_demand, "High demand only").changed();
            });
        });
        if changed {
            app.cfg.save();
        }
        muted(ui, "Checked whenever Heximons values refresh (every 15 minutes); each deal is announced once.");
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


    h2(ui, "Updates");
    ui::tile(ui, 620.0, |ui| {
        ui.horizontal(|ui| {
            muted(ui, &format!("Version {}", crate::update::CURRENT));
            muted(ui, &format!("from github.com/{}", crate::update::REPO));
        });
        muted(ui, "New versions download in the background and run from the next start.");
        if ui.checkbox(&mut app.cfg.auto_update, "Install new versions on their own").changed() {
            app.cfg.save();
            if app.cfg.auto_update && crate::update::enabled() {
                crate::update::check(&app.rt, app.egui.clone());
            }
        }
        ui.horizontal(|ui| {
            let status = crate::update::status();
            let busy = matches!(status, crate::update::Status::Checking | crate::update::Status::Downloading(_));
            if crate::update::enabled() {
                if ui.add_enabled(!busy, egui::Button::new("Check now")).clicked() {
                    crate::update::check(&app.rt, app.egui.clone());
                }
            } else {
                muted(ui, "Development build: updates are off.");
            }
            match status {
                crate::update::Status::Idle => {}
                crate::update::Status::Checking => muted(ui, "Checking..."),
                crate::update::Status::UpToDate => {
                    ui.label(RichText::new("Up to date.").color(GREEN));
                }
                crate::update::Status::Downloading(version) => muted(ui, &format!("Downloading {version}...")),
                crate::update::Status::Ready(version) => {
                    ui.label(RichText::new(format!("{version} installed.")).color(GREEN));
                    if ui.button("Restart now").clicked() {
                        crate::update::restart();
                    }
                }
                crate::update::Status::Failed(error) => {
                    ui.label(RichText::new(error).color(DANGER));
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
            "chat/v2 + the /chat SignalR hub — conversations, messages, live updates",
            "heximons.lol (a third-party values site) — item values, demand and trend, read from its public catalog page; no cookie or account data is sent there",
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
