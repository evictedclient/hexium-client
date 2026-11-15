//! Home: a dashboard about you, not about the API. A greeting, your balances,
//! what needs your attention, who is around, and what is on.

use eframe::egui::{self, RichText, Ui};

use crate::app::{App, Load, View};
use crate::ui::{
    self, accent, banner, display, eyebrow, failed, h2, muted, semibold, waiting, Ctx, GOLD, GREEN,
    INK, MUTED,
};

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    app.load_friends();
    app.load_events();

    if let Some(alert) = app.alert.clone() {
        banner(ui, &alert, accent());
        ui.add_space(10.0);
    }
    if let Some(maintenance) = app.maintenance.clone() {
        banner(ui, &maintenance, GOLD);
        ui.add_space(10.0);
    }

    let session = match app.session.clone() {
        Load::Ready(session) => session,
        Load::Err(error) => {
            failed(ui, &error);
            if ui.button("Retry").clicked() {
                app.session = Load::Idle;
                app.load_session();
            }
            return;
        }
        _ => {
            waiting(ui, "your account");
            return;
        }
    };

    // ---- hero
    let full = ui.available_width();
    ui::card_frame()
        .inner_margin(egui::Margin::same(20.0))
        .show(ui, |ui| {
            ui.set_width(full - 42.0);
            ui.horizontal(|ui| {
                app.thumbs
                    .show(ui, cx, session.headshot.as_deref(), egui::vec2(96.0, 96.0));
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.add_space(6.0);
                    eyebrow(ui, &today());
                    ui.label(
                        RichText::new(format!("{}, {}", greeting(), session.user.display_name))
                            .font(display(34.0))
                            .color(INK),
                    );
                    ui.horizontal(|ui| {
                        muted(ui, &format!("@{}", session.user.name));
                        if session.user.is_staff {
                            ui::pill(ui, "STAFF", GOLD);
                        }
                        if session.user.is_admin {
                            ui::pill(ui, "ADMIN", accent());
                        }
                        if session.user.has_verified_badge || session.user.is_verified {
                            ui::pill(ui, "VERIFIED", GREEN);
                        }
                    });
                });
                // Push the balances to the right edge of the card.
                ui.add_space((ui.available_width() - 250.0).max(16.0));
                figure(ui, "Robux", &ui::money(session.currency.robux), GOLD);
                ui.add_space(22.0);
                figure(ui, "Tickets", &ui::money(session.currency.tix), GREEN);
            });
        });

    // ---- what needs you
    h2(ui, "For you");
    let counts = session.counts;
    let mut target: Option<View> = None;
    ui::wrap(ui, |ui| {
        for (label, count, waiting_word, view) in [
            ("Messages", counts.unread, "unread", View::Messages),
            ("Friend requests", counts.friend_requests, "waiting", View::Friends),
            ("Trade offers", counts.trades, "to answer", View::Trades),
        ] {
            let response = ui::clickable_card(ui, 196.0, |ui| {
                eyebrow(ui, label);
                ui.horizontal(|ui| {
                    let color = if count > 0 { accent() } else { MUTED };
                    ui.label(RichText::new(ui::money(count as i64)).font(display(30.0)).color(color));
                    ui.label(
                        RichText::new(if count > 0 { waiting_word } else { "all caught up" })
                            .size(12.5)
                            .color(MUTED),
                    );
                });
            });
            if response.clicked() {
                target = Some(view);
            }
        }
    });
    if let Some(view) = target {
        ui::go(app, view);
        return;
    }

    // ---- who's around
    h2(ui, "Friends online");
    let mut open_profile: Option<u64> = None;
    match app.friends.data.clone() {
        Load::Ready(bundle) => {
            let online: Vec<_> = bundle
                .friends
                .iter()
                .filter(|friend| friend.is_online)
                .cloned()
                .collect();
            if online.is_empty() {
                muted(ui, "Nobody is around right now.");
            } else {
                ui::wrap(ui, |ui| {
                    for friend in &online {
                        let icon = bundle.icons.get(&friend.id).cloned();
                        let response = ui::clickable_card(ui, 196.0, |ui| {
                            ui.horizontal(|ui| {
                                app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(40.0, 40.0));
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(&friend.display_name).font(semibold(13.5)),
                                    );
                                    ui.horizontal(|ui| {
                                        ui::presence_dot(ui, true);
                                        ui.label(
                                            RichText::new(crate::api::models::presence_label(
                                                friend.presence,
                                                friend.is_online,
                                            ))
                                            .size(12.0)
                                            .color(GREEN),
                                        );
                                    });
                                });
                            });
                        });
                        if response.clicked() {
                            open_profile = Some(friend.id);
                        }
                    }
                });
            }
        }
        Load::Err(error) => failed(ui, &format!("Could not load friends: {error}")),
        _ => waiting(ui, "friends"),
    }
    if let Some(id) = open_profile {
        app.open_profile(id);
        return;
    }

    // ---- what's on
    h2(ui, "Happening on Hexium");
    match app.home.events.clone() {
        Load::Ready(events) => {
            if events.is_empty() {
                muted(ui, "Nothing posted right now.");
            } else {
                for event in events.iter().take(6) {
                    let width = (ui.available_width() - 26.0).max(320.0);
                    ui::card_frame().show(ui, |ui| {
                        ui.set_width(width);
                        ui.label(RichText::new(&event.title).font(semibold(15.0)));
                        if !event.description.is_empty() {
                            ui.label(RichText::new(trim(&event.description, 360)).size(13.0).color(MUTED));
                        }
                        if let Some(url) = &event.url {
                            ui.hyperlink_to("Read more", url);
                        }
                    });
                    ui.add_space(8.0);
                }
            }
        }
        Load::Err(error) => failed(ui, &format!("Could not load events: {error}")),
        _ => waiting(ui, "events"),
    }
}

fn figure(ui: &mut Ui, label: &str, value: &str, color: egui::Color32) {
    ui.vertical(|ui| {
        eyebrow(ui, label);
        ui.add(
            egui::Label::new(RichText::new(value).font(display(32.0)).color(color))
                .wrap_mode(egui::TextWrapMode::Extend),
        );
    });
}

/// Repeated links and very long posts are cut so one event cannot fill the page.
fn trim(text: &str, limit: usize) -> String {
    let flat = text.trim();
    if flat.chars().count() <= limit {
        return flat.to_string();
    }
    let cut: String = flat.chars().take(limit).collect();
    format!("{}…", cut.trim_end())
}

fn greeting() -> &'static str {
    match local_time().map(|time| time.hour) {
        Some(5..=11) => "Good morning",
        Some(12..=17) => "Good afternoon",
        Some(18..=22) => "Good evening",
        Some(_) => "Up late",
        None => "Welcome back",
    }
}

fn today() -> String {
    const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    const MONTHS: [&str; 12] = [
        "January", "February", "March", "April", "May", "June", "July", "August", "September",
        "October", "November", "December",
    ];
    match local_time() {
        Some(time) => format!(
            "{} {} {}",
            DAYS.get(time.weekday as usize).copied().unwrap_or(""),
            time.day,
            MONTHS.get(time.month.saturating_sub(1) as usize).copied().unwrap_or("")
        ),
        None => "Welcome".to_string(),
    }
}

struct LocalTime {
    hour: u16,
    day: u16,
    month: u16,
    weekday: u16,
}

/// The wall clock, from Windows itself so the greeting matches the taskbar.
#[cfg(windows)]
fn local_time() -> Option<LocalTime> {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    extern "system" {
        fn GetLocalTime(time: *mut SystemTime);
    }
    let mut time = SystemTime::default();
    unsafe { GetLocalTime(&mut time) };
    Some(LocalTime {
        hour: time.hour,
        day: time.day,
        month: time.month,
        weekday: time.day_of_week,
    })
}

#[cfg(not(windows))]
fn local_time() -> Option<LocalTime> {
    None
}
