use eframe::egui::{self, RichText, Ui};

use crate::app::{App, Load, WriteAction};
use crate::ui::{self, accent, failed, h1, muted, waiting, Ctx, GOLD, GREEN, MUTED};

pub fn show(app: &mut App, cx: &Ctx, ui: &mut Ui) {
    app.load_catalog_nav();
    app.load_catalog();

    h1(ui, "Catalog");
    muted(ui, "Clothing, accessories and collectibles.");
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(&mut app.catalog.keyword)
                .hint_text("Search the catalog…")
                .desired_width(320.0),
        );
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

    if let Some(item) = app.catalog.selected.clone() {
        let mut open = true;
        egui::Window::new(format!("Item · {}", item.name))
            .id(egui::Id::new(("item", item.id)))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(430.0)
            .show(ui.ctx(), |ui| match app.catalog.detail.clone() {
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
                        resellers(app, cx, ui, detail.id);
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
                            if listing.seller_id != 0 && ui.link(RichText::new(name).size(12.5)).clicked() {
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
