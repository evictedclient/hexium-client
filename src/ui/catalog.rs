use eframe::egui::{self, RichText, Ui};

use crate::app::{App, Load, WriteAction};
use crate::ui::{self, accent, failed, h1, muted, waiting, Ctx, GOLD, GREEN, MUTED};

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    app.load_catalog_nav();
    app.load_catalog();

    h1(ui, "Catalog");
    muted(ui, "Clothing, accessories and collectibles.");
    ui.add_space(8.0);
    watchlist(app, cx, ui);

    ui.horizontal(|ui| {
        if ui.selectable_label(!app.catalog.deals_mode, "Browse").clicked() {
            app.catalog.deals_mode = false;
        }
        if ui.selectable_label(app.catalog.deals_mode, "Deals").clicked() {
            app.catalog.deals_mode = true;
        }
    });
    ui.add_space(4.0);
    if app.catalog.deals_mode {
        deals(app, cx, ui);
        return;
    }

    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(&mut app.catalog.keyword)
                .hint_text("Search the catalog…")
                .desired_width(320.0),
        );
        if std::mem::take(&mut app.focus_search) {
            response.request_focus();
        }
        let submitted =
            response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        if ui.button("Search").clicked() || submitted {
            app.reload_catalog();
        }
    });

    if let Load::Ready(entries) = app.catalog.nav.clone() {
        ui::wrap(ui, |ui| {
            for (value, label) in entries {
                let selected = app.catalog.category == value;
                if ui.selectable_label(selected, label).clicked() && !selected {
                    app.catalog.category = value;
                    app.reload_catalog();
                }
            }
        });
    } else if let Load::Err(error) = app.catalog.nav.clone() {
        muted(ui, &format!("Category list unavailable: {error}"));
    }

    ui.add_space(8.0);
    match app.catalog.items.clone() {
        Load::Ready(bundle) => {
            if bundle.items.is_empty() {
                muted(ui, "Nothing in this category.");
            } else {
                muted(ui, &format!("{} items loaded", bundle.items.len()));
                ui.add_space(4.0);
                ui::wrap(ui, |ui| {
                    for item in &bundle.items {
                        let icon = bundle.icons.get(&item.id).cloned();
                        let price = price_text(item);

                        let kind = if item.asset_type.is_empty() {
                            item.item_type.as_str()
                        } else {
                            item.asset_type.as_str()
                        };
                        let name = if item.name.is_empty() {
                            format!("Asset #{}", item.id)
                        } else {
                            item.name.clone()
                        };
                        let selected = app
                            .catalog
                            .selected
                            .as_ref()
                            .map(|selected| selected.id == item.id)
                            .unwrap_or(false);

                        let card = ui::tile_button(ui, 168.0, selected, |ui| {
                            app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(144.0, 144.0));
                            ui.label(RichText::new(&name).font(ui::semibold(12.5)));
                            muted(ui, kind);
                            ui.label(RichText::new(price).size(12.5).color(GOLD));
                            ui.horizontal(|ui| {
                                if let Some(tag) = item.limited_tag() {
                                    ui::pill(ui, tag, GOLD);
                                }
                                if item.off_sale {
                                    ui::pill(ui, "OFF SALE", MUTED);
                                }
                            });
                        });
                        if card.on_hover_text("Open details").clicked() && !selected {
                            app.catalog.selected = Some(item.clone());
                            app.catalog.detail = Load::Idle;
                            app.load_catalog_detail(item.id);
                        }
                    }
                });

                if bundle.next.is_some() {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if app.catalog.loading_more {
                            waiting(ui, "the next page");
                        } else if ui.button("Load more").clicked() {
                            app.load_more_catalog();
                        }
                    });
                }
                if let Some(error) = &app.catalog.more_error {
                    failed(ui, error);
                }
            }
        }
        Load::Err(error) => {
            failed(ui, &error);
            if ui.button("Retry").clicked() {
                app.reload_catalog();
            }
        }
        _ => waiting(ui, "catalog"),
    }
}

/// The selected item, in the docked panel.
pub fn dock(app: &mut App, cx: &Ctx, ctx: &egui::Context) {
    if let Some(item) = app.catalog.selected.clone() {
        let mut open = true;
        ui::dock(ctx, "item", 460.0, egui::Id::new(("item", item.id)), format!("Item · {}", item.name), &mut open, |ui| match app.catalog.detail.clone() {
                Load::Ready(detail) => {
                    ui.label(RichText::new(&detail.name).size(16.0).strong());
                    muted(
                        ui,
                        if detail.asset_type.is_empty() {
                            &detail.item_type
                        } else {
                            &detail.asset_type
                        },
                    );

                    if let Some(tag) = detail.limited_tag() {
                        ui::pill(ui, tag, GOLD);
                    }
                    ui::analyze::value_line(ui, &app.values, detail.id);
                    if detail.limited && ui.link(RichText::new("View on Heximons").size(11.5)).clicked() {
                        cx.open(&crate::values::item_url(detail.id));
                    }
                    if detail.limited {
                        own_value(app, ui, detail.id, &detail.name);
                    }
                    if detail.resold {
                        ui.label(RichText::new(resale_text(&detail)).color(GOLD));
                    } else if let Some(price) = detail.price {
                        ui.label(RichText::new(format!("{} R$", ui::money(price))).color(GOLD));
                    }
                    if let Some(tix) = detail.price_tix {
                        ui.label(RichText::new(format!("{} Tix", ui::money(tix))).color(GREEN));
                    }

                    if !detail.creator.is_empty() {
                        muted(ui, &format!("by {}", detail.creator));
                    }
                    if !detail.description.is_empty() {
                        ui.add_space(4.0);
                        ui.label(RichText::new(&detail.description).size(12.0));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| match app.catalog.favourite.clone() {
                        Load::Ready(favourite) => {
                            let label = if favourite { "★ Favourited · remove" } else { "☆ Favourite" };
                            if ui.button(label).clicked() {
                                app.ask(WriteAction::Favourite {
                                    asset_id: detail.id,
                                    name: detail.name.clone(),
                                    favourite: !favourite,
                                });
                            }
                        }
                        Load::Err(error) => muted(ui, &format!("favourite state unavailable: {error}")),
                        _ => {
                            ui.add(egui::Spinner::new().size(12.0));
                        }
                    });
                    if detail.limited {
                        ui.add_space(8.0);
                        watch_controls(app, ui, detail.id, &detail.name);
                        ui.add_space(8.0);
                        ui::eyebrow(ui, "Price history");
                        match app.catalog.resale.clone() {
                            Load::Ready(data) if data.asset_id == detail.id => {
                                ui::chart::summary(ui, &data);
                                ui.add_space(4.0);
                                ui::chart::show(ui, &data);
                            }
                            Load::Err(error) => failed(ui, &format!("Could not load the price history: {error}")),
                            _ => waiting(ui, "price history"),
                        }
                        ui.add_space(8.0);
                        resellers(app, cx, ui, detail.id);
                        ui.add_space(8.0);
                        owners(app, cx, ui, detail.id);
                    }
                }
                Load::Err(error) => {
                    failed(ui, &error);
                    muted(
                        ui,
                        "This is the one POST in the client. It needs a valid CSRF token, so a \
                         stale session shows up here first.",
                    );
                }
                _ => waiting(ui, "item details"),
            });
        if !open {
            app.catalog.selected = None;
        }
    }
}

/// Everyone selling a copy of this limited, cheapest first. Read-only: buying
/// needs the site's purchase proof, which this client does not fake.
fn resellers(app: &mut App, cx: &Ctx, ui: &mut Ui, asset: u64) {
    let me = app.session.ready().map(|session| session.user.id).unwrap_or(0);
    let page = app.catalog.resellers.clone();
    ui.horizontal(|ui| {
        ui::eyebrow(ui, "Resellers");
        if let Load::Ready(page) = &page {
            let more = if page.next.is_some() { "+" } else { "" };
            muted(ui, &format!("{}{more}", page.items.len()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let busy = matches!(page, Load::Busy);
            if ui.add_enabled(!busy, egui::Button::new("Refresh").small()).clicked() {
                app.load_resellers(asset, None);
            }
        });
    });
    let page = match page {
        Load::Ready(page) if page.asset_id == asset => page,
        Load::Err(error) => {
            failed(ui, &format!("Could not load resellers: {error}"));
            return;
        }
        Load::Busy => {
            waiting(ui, "resellers");
            return;
        }
        _ => return,
    };
    if page.items.is_empty() {
        muted(ui, "Nobody is selling a copy right now.");
        return;
    }

    let mut open_profile: Option<u64> = None;
    egui::ScrollArea::vertical()
        .id_salt(("resellers", asset))
        .max_height(240.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (index, listing) in page.items.iter().enumerate() {
                egui::Frame::none()
                    .fill(if index == 0 { ui::tint(GOLD, 22) } else { ui::card() })
                    .stroke(egui::Stroke::new(1.0, if index == 0 { ui::tint(GOLD, 120) } else { ui::line() }))
                    .rounding(egui::Rounding::same(7.0))
                    .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let icon = page.icons.get(&listing.seller_id).cloned();
                            app.thumbs.show(ui, cx, icon.as_deref(), egui::vec2(24.0, 24.0));
                            let name = if listing.seller_name.is_empty() { "unknown" } else { listing.seller_name.as_str() };
                            if listing.seller_id != 0 && ui::player_card::link(ui, cx, listing.seller_id, RichText::new(name).size(12.5)).clicked() {
                                open_profile = Some(listing.seller_id);
                            } else if listing.seller_id == 0 {
                                ui.label(RichText::new(name).size(12.5));
                            }
                            if listing.seller_id == me && me != 0 {
                                ui::pill(ui, "YOU", accent());
                            }
                            if let Some(serial) = listing.serial {
                                ui.label(RichText::new(format!("#{serial}")).size(11.5).color(MUTED));
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    RichText::new(format!("{} R$", ui::money(listing.price as i64)))
                                        .font(ui::semibold(13.0))
                                        .color(GOLD),
                                );
                            });
                        });
                    });
                ui.add_space(2.0);
            }
            if let Some(cursor) = page.next.clone() {
                if app.catalog.resellers_more {
                    waiting(ui, "more resellers");
                } else if ui.button("Load more").clicked() {
                    app.load_resellers(asset, Some(cursor));
                }
            }
        });
    if let Some(id) = open_profile {
        app.open_profile(id);
    }
}

/// What a limited costs now: its cheapest reseller.
fn resale_text(item: &crate::api::models::CatalogItem) -> String {
    match item.resale_price {
        Some(price) => format!("From {} R$", ui::money(price)),
        None => "No resellers".to_string(),
    }
}

/// The price line on a catalog card.
fn price_text(item: &crate::api::models::CatalogItem) -> String {
    if item.resold {
        return resale_text(item);
    }
    match (item.price, item.price_tix) {
        (Some(robux), Some(tix)) => format!("{} R$ · {} Tix", ui::money(robux), ui::money(tix)),
        (Some(robux), None) => format!("{} R$", ui::money(robux)),
        (None, Some(tix)) => format!("{} Tix", ui::money(tix)),
        (None, None) => "—".to_string(),
    }
}

/// Watch this limited: alert when its cheapest reseller reaches a price.
fn watch_controls(app: &mut App, ui: &mut Ui, asset: u64, name: &str) {
    let watched = app.watched(asset).cloned();
    ui::eyebrow(ui, "Watchlist");
    ui.horizontal(|ui| {
        ui.label(RichText::new(if watched.is_some() { "Alert at" } else { "Alert me at" }).size(12.5));
        app.catalog.watch_input.retain(|character| character.is_ascii_digit());
        ui.add(
            egui::TextEdit::singleline(&mut app.catalog.watch_input)
                .hint_text("price")
                .char_limit(9)
                .desired_width(80.0),
        );
        ui.label(RichText::new("R$ or less").size(12.0).color(GOLD));
        let target: Option<u64> = app.catalog.watch_input.parse().ok().filter(|target| *target > 0);
        match &watched {
            Some(current) => {
                let changed = target.is_some_and(|target| target != current.target);
                if ui.add_enabled(changed, egui::Button::new("Save")).clicked() {
                    app.set_watch(asset, name, target);
                }
                if ui.button("Stop watching").clicked() {
                    app.set_watch(asset, name, None);
                }
            }
            None => {
                if ui.add_enabled(target.is_some(), egui::Button::new("☆ Watch")).clicked() {
                    app.set_watch(asset, name, target);
                }
            }
        }
    });
    if watched.is_some() {
        let line = match app.watch_prices.get(&asset) {
            Some(Some(price)) => format!("Cheapest now {} R$ · checked every 2 minutes", ui::money(*price as i64)),
            Some(None) => "Nobody is selling one · checked every 2 minutes".to_string(),
            None => "Checking soon…".to_string(),
        };
        muted(ui, &line);
    }
}

/// Watched limiteds, at the top of the Catalog screen.
fn watchlist(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    if app.cfg.watchlist.is_empty() {
        return;
    }
    let mut open: Option<(u64, String)> = None;
    let mut remove: Option<(u64, String)> = None;
    ui::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui::eyebrow(ui, "Watchlist");
            muted(ui, "You get a notification when the cheapest copy reaches your price.");
        });
        ui.add_space(2.0);
        for item in app.cfg.watchlist.clone() {
            ui.horizontal(|ui| {
                if ui::item_card::link(ui, cx, item.asset_id, RichText::new(&item.name).size(12.5)).clicked() {
                    open = Some((item.asset_id, item.name.clone()));
                }
                ui.label(
                    RichText::new(format!("at {} R$", ui::money(item.target as i64)))
                        .size(11.5)
                        .color(GOLD),
                );
                let (now, hit) = match app.watch_prices.get(&item.asset_id) {
                    Some(Some(price)) => (format!("now {} R$", ui::money(*price as i64)), *price <= item.target),
                    Some(None) => ("nobody selling".to_string(), false),
                    None => ("checking…".to_string(), false),
                };
                ui.label(RichText::new(now).size(11.5).color(if hit { GREEN } else { MUTED }));
                if hit {
                    ui::pill(ui, "AT TARGET", GREEN);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new(RichText::new("✕").size(12.0).color(MUTED)).frame(false))
                        .on_hover_text("Stop watching")
                        .clicked()
                    {
                        remove = Some((item.asset_id, item.name.clone()));
                    }
                });
            });
        }
    });
    ui.add_space(8.0);
    if let Some((asset, name)) = open {
        app.open_item(asset, &name);
    }
    if let Some((asset, name)) = remove {
        app.set_watch(asset, &name, None);
    }
}

/// Limiteds listed well below their Heximons value, biggest discount first.
fn deals(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    if app.catalog.deal_min <= 0.0 {
        app.catalog.deal_min = 20.0;
    }
    muted(
        ui,
        "Cheapest listing against value (the Heximons staff value, otherwise RAP). The top rows are checked against hexium.wtf's live prices.",
    );
    ui.horizontal(|ui| {
        ui.label("At least");
        ui.add(egui::Slider::new(&mut app.catalog.deal_min, 5.0..=80.0).suffix("% off").step_by(5.0));
        ui.checkbox(&mut app.catalog.deal_staff_only, "Staff values only");
    });
    ui.add_space(4.0);

    if app.values.items.is_empty() {
        match &app.values_error {
            Some(error) => failed(ui, &format!("No Heximons values yet: {error}")),
            None => waiting(ui, "values from Heximons"),
        }
        return;
    }
    let found = crate::values::deals(&app.values, app.catalog.deal_min, app.catalog.deal_staff_only);
    if found.is_empty() {
        muted(ui, "Nothing that far below value right now.");
        return;
    }
    muted(ui, &format!("{} deal(s)", found.len()));
    ui.add_space(2.0);

    let mut open: Option<(u64, String)> = None;
    let width = (ui.available_width() - 4.0).max(320.0);
    for (index, deal) in found.iter().enumerate() {
        // Live prices for the first rows only, to keep requests few.
        if index < 20 {
            app.check_deal(deal.asset_id);
        }
        let live = app.catalog.deal_live.get(&deal.asset_id).cloned();
        let thumb = app.values.get(deal.asset_id).and_then(|item| item.thumb.clone());
        let card = ui::tile_button(ui, width, false, |ui| {
            ui.horizontal(|ui| {
                app.thumbs.show(ui, cx, thumb.as_deref(), egui::vec2(48.0, 48.0));
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&deal.name).font(ui::semibold(13.0)));
                        ui::pill(ui, &format!("-{:.0}%", deal.discount), GREEN);
                        if !deal.staff_valued {
                            ui.label(RichText::new("RAP-based").size(10.5).color(MUTED))
                                .on_hover_text("No staff value, so this compares against RAP.");
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{} R$", ui::money(deal.price as i64)))
                                .font(ui::semibold(12.5))
                                .color(GOLD),
                        );
                        ui.label(
                            RichText::new(format!("value {}", ui::money(deal.value as i64)))
                                .size(11.5)
                                .color(MUTED),
                        );
                        match live {
                            Some(Load::Ready(Some(price))) if price == deal.price => {
                                ui.label(RichText::new("· still listed").size(11.0).color(GREEN));
                            }
                            Some(Load::Ready(Some(price))) => {
                                let still = price < deal.value;
                                ui.label(
                                    RichText::new(format!("· now {} R$", ui::money(price as i64)))
                                        .size(11.0)
                                        .color(if still { GREEN } else { ui::DANGER }),
                                );
                            }
                            Some(Load::Ready(None)) => {
                                ui.label(RichText::new("· sold out").size(11.0).color(ui::DANGER));
                            }
                            Some(Load::Busy) => {
                                ui.label(RichText::new("· checking…").size(11.0).color(MUTED));
                            }
                            _ => {}
                        }
                    });
                    ui::analyze::value_line(ui, &app.values, deal.asset_id);
                });
            });
        });
        if card.on_hover_text("Open details").clicked() {
            open = Some((deal.asset_id, deal.name.clone()));
        }
    }
    if let Some((asset, name)) = open {
        app.open_item(asset, &name);
        app.catalog.deals_mode = true;
    }
}

/// Everyone who owns a copy, with a Trade button each (loaded on request).
fn owners(app: &mut App, cx: &Ctx, ui: &mut Ui, asset: u64) {
    ui.horizontal(|ui| {
        ui::eyebrow(ui, "Owners");
        let loaded = matches!(&app.catalog.owners, Some((shown, _)) if *shown == asset);
        if ui.small_button(if loaded { "Refresh" } else { "Show owners" }).clicked() {
            app.load_owners(asset);
        }
    });
    let Some((shown, state)) = app.catalog.owners.clone() else { return };
    if shown != asset {
        return;
    }
    let me = app.session.ready().map(|session| session.user.id).unwrap_or(0);
    match state {
        Load::Ready(list) => {
            muted(ui, &format!("{} copies", list.len()));
            let mut trade: Option<crate::api::endpoints::AssetOwner> = None;
            let mut profile: Option<u64> = None;
            egui::ScrollArea::vertical()
                .id_salt(("owners", asset))
                .max_height(220.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for owner in &list {
                        ui.horizontal(|ui| {
                            if ui::player_card::link(ui, cx, owner.user_id, &owner.name).clicked() {
                                profile = Some(owner.user_id);
                            }
                            if let Some(serial) = owner.serial {
                                ui.label(RichText::new(format!("#{serial}")).size(11.5).color(GOLD));
                            }
                            let since: String = owner.updated.chars().take(10).collect();
                            ui.label(RichText::new(format!("since {since}")).size(11.0).color(MUTED));
                            if owner.user_id == me {
                                ui::pill(ui, "YOU", accent());
                            } else if owner.user_id > 1 {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("Trade…").clicked() {
                                        trade = Some(owner.clone());
                                    }
                                });
                            }
                        });
                    }
                });
            if let Some(id) = profile {
                app.open_profile(id);
            }
            if let Some(owner) = trade {
                app.open_trade_builder(owner.user_id, &owner.name, None, Vec::new(), vec![owner.user_asset_id], (None, None));
            }
        }
        Load::Err(error) => failed(ui, &format!("Could not load owners: {error}")),
        _ => waiting(ui, "owners"),
    }
}

/// Your own value for this item: replaces Heximons' (or adds one) everywhere.
fn own_value(app: &mut App, ui: &mut Ui, asset: u64, name: &str) {
    let current = app.cfg.value_overrides.iter().find(|own| own.asset_id == asset).map(|own| own.value);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Your value").size(12.0));
        app.catalog.override_input.retain(|character| character.is_ascii_digit());
        let response = ui.add(
            egui::TextEdit::singleline(&mut app.catalog.override_input)
                .hint_text("R$")
                .char_limit(10)
                .desired_width(80.0),
        );
        let typed: Option<u64> = app.catalog.override_input.parse().ok().filter(|value| *value > 0);
        let enter = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        let changed = typed.is_some() && typed != current;
        if (ui.add_enabled(changed, egui::Button::new("Set")).clicked() || (enter && changed)) && typed.is_some() {
            app.set_override(asset, name, typed);
        }
        if current.is_some() && ui.button("Clear").on_hover_text("Use Heximons' value again").clicked() {
            app.set_override(asset, name, None);
            app.catalog.override_input.clear();
        }
    })
    .response
    .on_hover_text("Used for totals, trade verdicts, Deals and suggestions instead of Heximons' value.");
}
