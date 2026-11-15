//! Shared shell: palette, type, widgets, the frame around every screen, the
//! sign-in gate, and the thumbnail cache that feeds `egui::Image`.
//!
//! The look is deliberately flat: solid surfaces, 1px lines, one accent colour
//! the user picks, and a hexagon motif taken from the name. No gradients, glows
//! or drop shadows anywhere.

pub mod avatar;
pub mod catalog;
pub mod friends;
pub mod games;
pub mod groups;
pub mod home;
pub mod inventory;
pub mod messages;
pub mod money;
pub mod profile;
pub mod settings;
pub mod trades;

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, RwLock};

use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Sense, Stroke, Ui};

use crate::api::ApiClient;
use crate::app::{App, Load, Msg, View, WriteAction};

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

pub const INK: Color32 = Color32::from_rgb(236, 237, 239);
pub const MUTED: Color32 = Color32::from_rgb(141, 147, 157);
pub const FAINT: Color32 = Color32::from_rgb(98, 104, 114);
/// Robux.
pub const GOLD: Color32 = Color32::from_rgb(240, 185, 74);
/// Tickets, online, success.
pub const GREEN: Color32 = Color32::from_rgb(67, 200, 138);
pub const DANGER: Color32 = Color32::from_rgb(240, 104, 106);

/// The accent colours on offer in Settings: (key, label, colour).
pub const ACCENTS: [(&str, &str, Color32); 6] = [
    ("coral", "Coral", Color32::from_rgb(255, 122, 89)),
    ("sky", "Sky", Color32::from_rgb(77, 163, 255)),
    ("mint", "Mint", Color32::from_rgb(47, 211, 167)),
    ("lilac", "Lilac", Color32::from_rgb(169, 139, 255)),
    ("rose", "Rose", Color32::from_rgb(255, 92, 143)),
    ("honey", "Honey", Color32::from_rgb(245, 184, 61)),
];

/// Surfaces for the two backgrounds plus the chosen accent. Screens read it
/// through the small functions below, so a change in Settings applies at once.
#[derive(Clone, Copy)]
struct Palette {
    bg: Color32,
    panel: Color32,
    card: Color32,
    raised: Color32,
    line: Color32,
    accent: Color32,
}

const MIDNIGHT: Palette = Palette {
    bg: Color32::from_rgb(14, 15, 17),
    panel: Color32::from_rgb(20, 21, 24),
    card: Color32::from_rgb(26, 28, 32),
    raised: Color32::from_rgb(35, 38, 43),
    line: Color32::from_rgb(39, 42, 48),
    accent: Color32::from_rgb(255, 122, 89),
};

const SLATE: Palette = Palette {
    bg: Color32::from_rgb(22, 24, 28),
    panel: Color32::from_rgb(28, 31, 36),
    card: Color32::from_rgb(35, 39, 45),
    raised: Color32::from_rgb(44, 49, 56),
    line: Color32::from_rgb(51, 56, 66),
    accent: Color32::from_rgb(255, 122, 89),
};

static PALETTE: RwLock<Palette> = RwLock::new(MIDNIGHT);

fn palette() -> Palette {
    PALETTE.read().map(|palette| *palette).unwrap_or(MIDNIGHT)
}

pub fn accent() -> Color32 {
    palette().accent
}
pub fn bg() -> Color32 {
    palette().bg
}
pub fn panel() -> Color32 {
    palette().panel
}
pub fn card() -> Color32 {
    palette().card
}
pub fn raised() -> Color32 {
    palette().raised
}
pub fn line() -> Color32 {
    palette().line
}

/// `color` mixed into the card surface at `amount` (0-255), as a solid colour,
/// so tinted chips and selections look the same over any surface.
pub fn tint(color: Color32, amount: u8) -> Color32 {
    let base = card();
    let t = amount as f32 / 255.0;
    let mix = |from: u8, to: u8| (from as f32 + (to as f32 - from as f32) * t).round() as u8;
    Color32::from_rgb(
        mix(base.r(), color.r()),
        mix(base.g(), color.g()),
        mix(base.b(), color.b()),
    )
}

/// Dark text for use on top of the accent.
fn on_accent() -> Color32 {
    Color32::from_rgb(18, 18, 20)
}

// ---------------------------------------------------------------------------
// Context for screens
// ---------------------------------------------------------------------------

/// Everything a screen needs that is not app state.
pub struct Ctx {
    pub api: Arc<ApiClient>,
    pub tx: std::sync::mpsc::Sender<Msg>,
    pub rt: tokio::runtime::Handle,
    pub egui: egui::Context,
}

impl Ctx {
    pub fn spawn<F>(&self, future: F)
    where
        F: Future<Output = Msg> + Send + 'static,
    {
        let tx = self.tx.clone();
        let ctx = self.egui.clone();
        self.rt.spawn(async move {
            let message = future.await;
            if tx.send(message).is_ok() {
                ctx.request_repaint();
            }
        });
    }

    pub fn open(&self, url: &str) {
        self.egui.open_url(egui::OpenUrl::new_tab(url));
    }
}

// ---------------------------------------------------------------------------
// Thumbnails: fetched with the same HTTP client, cached as decoded bytes.
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct Thumbs {
    entries: HashMap<String, ThumbEntry>,
}

enum ThumbEntry {
    Pending,
    Ready(Arc<[u8]>),
    Failed,
}

impl Thumbs {
    pub fn apply(&mut self, url: String, bytes: Option<Arc<[u8]>>) {
        let entry = match bytes {
            Some(bytes) => ThumbEntry::Ready(bytes),
            None => ThumbEntry::Failed,
        };
        self.entries.insert(url, entry);
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &Ctx, url: Option<&str>, size: egui::Vec2) -> egui::Response {
        let Some(url) = url.filter(|url| !url.is_empty()) else {
            return placeholder(ui, size);
        };
        let cached = match self.entries.get(url) {
            Some(ThumbEntry::Ready(bytes)) => Some(bytes.clone()),
            Some(_) => None,
            None => {
                self.entries.insert(url.to_string(), ThumbEntry::Pending);
                let api = cx.api.clone();
                let target = url.to_string();
                cx.spawn(async move {
                    let bytes = api
                        .fetch_image(&target)
                        .await
                        .ok()
                        .map(|bytes| Arc::<[u8]>::from(bytes));
                    Msg::Thumb(target, bytes)
                });
                None
            }
        };

        match cached {
            Some(bytes) => {
                let uri = format!("bytes://thumb/{:x}", fnv(url));
                ui.add(
                    egui::Image::from_bytes(uri, bytes)
                        .fit_to_exact_size(size)
                        .rounding(egui::Rounding::same((size.x.min(size.y) * 0.12).min(10.0))),
                )
            }
            None => placeholder(ui, size),
        }
    }
}

fn placeholder(ui: &mut Ui, size: egui::Vec2) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let radius = (size.x.min(size.y) * 0.12).min(10.0);
    ui.painter().rect_filled(rect, egui::Rounding::same(radius), raised());
    // A faint hexagon so empty slots read as "image goes here", not as a bug.
    let side = size.x.min(size.y) * 0.22;
    if side >= 5.0 {
        hexagon(ui.painter(), rect.center(), side, None, Some(Stroke::new(1.0, line())));
    }
    response
}

fn fnv(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

// ---------------------------------------------------------------------------
// Type
// ---------------------------------------------------------------------------

/// Bahnschrift (falls back to the body face) for titles and big numbers.
pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("display".into()))
}

/// Segoe UI Semibold (falls back to the body face) for emphasis.
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

/// Picks the type. Segoe UI for text, Semibold for emphasis, Bahnschrift for
/// headings and figures, then Segoe UI Symbol and Emoji as fallbacks because game
/// and item names are full of both. Every family is always defined, so a
/// machine without these fonts gets egui's defaults instead of a panic.
pub fn install_fonts(ctx: &egui::Context) {
    let directory = std::env::var_os("WINDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"))
        .join("Fonts");
    let mut fonts = egui::FontDefinitions::default();
    let mut load = |name: &str, file: &str| -> bool {
        match std::fs::read(directory.join(file)) {
            Ok(bytes) => {
                fonts
                    .font_data
                    .insert(name.to_owned(), egui::FontData::from_owned(bytes));
                true
            }
            Err(_) => false,
        }
    };
    let body = load("segoe", "segoeui.ttf");
    let strong = load("segoe-semibold", "seguisb.ttf");
    let headline = load("bahnschrift", "bahnschrift.ttf");
    let symbols = load("system-symbols", "seguisym.ttf");
    let emoji = load("system-emoji", "seguiemj.ttf");

    let defaults = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut tail = Vec::new();
    if symbols {
        tail.push("system-symbols".to_owned());
    }
    if emoji {
        tail.push("system-emoji".to_owned());
    }
    let build = |front: Vec<&str>| -> Vec<String> {
        let mut family: Vec<String> = front.into_iter().map(str::to_owned).collect();
        family.extend(defaults.iter().cloned());
        family.extend(tail.iter().cloned());
        family
    };

    let proportional = build(if body { vec!["segoe"] } else { vec![] });
    let mut semibold_front = Vec::new();
    if strong {
        semibold_front.push("segoe-semibold");
    }
    if body {
        semibold_front.push("segoe");
    }
    let semibold_family = build(semibold_front);
    let mut display_front = Vec::new();
    if headline {
        display_front.push("bahnschrift");
    }
    if strong {
        display_front.push("segoe-semibold");
    }
    let display_family = build(display_front);

    fonts.families.insert(FontFamily::Proportional, proportional);
    fonts
        .families
        .insert(FontFamily::Name("semibold".into()), semibold_family);
    fonts
        .families
        .insert(FontFamily::Name("display".into()), display_family);
    if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
        mono.extend(tail.iter().cloned());
    }
    ctx.set_fonts(fonts);
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

pub fn accent_for(key: &str) -> Color32 {
    ACCENTS
        .iter()
        .find(|(name, _, _)| *name == key)
        .map(|(_, _, color)| *color)
        .unwrap_or(ACCENTS[0].2)
}

pub fn apply_theme(ctx: &egui::Context, background: &str, accent_key: &str) {
    let mut chosen = if background == "slate" { SLATE } else { MIDNIGHT };
    chosen.accent = accent_for(accent_key);
    if let Ok(mut palette) = PALETTE.write() {
        *palette = chosen;
    }
    let p = chosen;

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = p.bg;
    visuals.window_fill = p.panel;
    visuals.extreme_bg_color = Color32::from_rgb(10, 11, 13);
    visuals.faint_bg_color = p.card;
    visuals.code_bg_color = p.card;
    visuals.override_text_color = Some(INK);
    visuals.hyperlink_color = p.accent;
    visuals.selection.bg_fill = tint(p.accent, 52);
    visuals.selection.stroke = Stroke::new(1.0, p.accent);
    visuals.window_rounding = egui::Rounding::same(12.0);
    visuals.menu_rounding = egui::Rounding::same(10.0);
    visuals.window_stroke = Stroke::new(1.0, p.line);
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.popup_shadow = egui::epaint::Shadow::NONE;
    visuals.striped = true;

    let rounding = egui::Rounding::same(8.0);
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = p.card;
    widgets.noninteractive.weak_bg_fill = p.card;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, INK);
    widgets.noninteractive.rounding = rounding;

    widgets.inactive.bg_fill = p.raised;
    widgets.inactive.weak_bg_fill = p.raised;
    widgets.inactive.bg_stroke = Stroke::NONE;
    widgets.inactive.fg_stroke = Stroke::new(1.0, INK);
    widgets.inactive.rounding = rounding;
    widgets.inactive.expansion = 0.0;

    let hover_fill = Color32::from_rgb(
        p.raised.r().saturating_add(10),
        p.raised.g().saturating_add(10),
        p.raised.b().saturating_add(12),
    );
    widgets.hovered.bg_fill = hover_fill;
    widgets.hovered.weak_bg_fill = hover_fill;
    widgets.hovered.bg_stroke = Stroke::new(1.0, tint(p.accent, 120));
    widgets.hovered.fg_stroke = Stroke::new(1.0, INK);
    widgets.hovered.rounding = rounding;
    widgets.hovered.expansion = 0.0;

    widgets.active.bg_fill = tint(p.accent, 70);
    widgets.active.weak_bg_fill = tint(p.accent, 70);
    widgets.active.bg_stroke = Stroke::new(1.0, p.accent);
    widgets.active.fg_stroke = Stroke::new(1.0, INK);
    widgets.active.rounding = rounding;
    widgets.active.expansion = 0.0;

    widgets.open = widgets.hovered;

    ctx.set_visuals(visuals);
    ctx.style_mut(|style| {
        use egui::TextStyle;
        style.text_styles = [
            (TextStyle::Heading, display(24.0)),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(13.5, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 28.0;
        style.spacing.window_margin = egui::Margin::same(18.0);
        style.spacing.scroll = egui::style::ScrollStyle::thin();
    });
}

// ---------------------------------------------------------------------------
// Small building blocks
// ---------------------------------------------------------------------------

/// Page title, in the display face.
pub fn h1(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).font(display(30.0)).color(INK));
}

/// Section title.
pub fn h2(ui: &mut Ui, text: &str) {
    ui.add_space(14.0);
    ui.label(RichText::new(text).font(semibold(16.0)).color(INK));
    ui.add_space(2.0);
}

/// Small, spaced-out capitals for labels above figures and nav groups.
pub fn eyebrow(ui: &mut Ui, text: &str) {
    ui.label(eyebrow_job(text, MUTED));
}

fn eyebrow_job(text: &str, color: Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &text.to_uppercase(),
        0.0,
        egui::text::TextFormat {
            font_id: semibold(10.5),
            color,
            extra_letter_spacing: 1.1,
            ..Default::default()
        },
    );
    job
}

pub fn muted(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(12.5).color(MUTED));
}

pub fn waiting(ui: &mut Ui, what: &str) {
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(13.0).color(accent()));
        ui.label(RichText::new(format!("Loading {what}…")).size(12.5).color(MUTED));
    });
}

pub fn failed(ui: &mut Ui, error: &str) {
    ui.label(RichText::new(error).size(12.5).color(DANGER));
}

/// A small filled chip: tinted background, coloured text.
pub fn pill(ui: &mut Ui, text: &str, color: Color32) {
    egui::Frame::none()
        .fill(tint(color, 38))
        .rounding(egui::Rounding::same(20.0))
        .inner_margin(egui::Margin::symmetric(7.0, 1.5))
        .show(ui, |ui| {
            ui.label(RichText::new(text).font(semibold(10.0)).color(color));
        });
}

/// The card surface every screen builds on.
pub fn card_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(card())
        .stroke(Stroke::new(1.0, line()))
        .rounding(egui::Rounding::same(10.0))
        .inner_margin(egui::Margin::same(12.0))
}

/// The accent-filled button for the one action that matters on a surface.
pub fn primary_button(ui: &mut Ui, text: &str) -> egui::Response {
    let button = egui::Button::new(RichText::new(text).font(semibold(13.5)).color(on_accent()))
        .fill(accent())
        .stroke(Stroke::NONE);
    ui.add(button).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A pointy-top hexagon, the brand mark. Used for the logo, nav bullets and
/// empty image slots.
pub fn hexagon(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    fill: Option<Color32>,
    stroke: Option<Stroke>,
) {
    let points: Vec<egui::Pos2> = (0..6)
        .map(|corner| {
            let angle = std::f32::consts::FRAC_PI_3 * corner as f32 - std::f32::consts::FRAC_PI_2;
            center + radius * egui::vec2(angle.cos(), angle.sin())
        })
        .collect();
    match (fill, stroke) {
        (Some(fill), stroke) => {
            painter.add(egui::Shape::convex_polygon(
                points,
                fill,
                stroke.unwrap_or(Stroke::NONE),
            ));
        }
        (None, Some(stroke)) => {
            painter.add(egui::Shape::closed_line(points, stroke));
        }
        (None, None) => {}
    }
}

/// A card that acts as a button: whole surface clickable, accent outline on hover.
pub fn clickable_card<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> egui::Response {
    // Same wrapping rule as `tile`, so a row of these never runs off-screen.
    let outer = width + card_frame().total_margin().sum().x;
    let row_started = ui.cursor().left() > ui.max_rect().left() + 1.0;
    if ui.layout().main_wrap && row_started && ui.available_size_before_wrap().x < outer {
        ui.end_row();
    }
    let shown = card_frame().show(ui, |ui| {
        ui.set_width(width);
        ui.vertical(add);
    });
    let response = shown
        .response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            egui::Rounding::same(10.0),
            Stroke::new(1.0, accent()),
        );
    }
    response
}

/// A row that wraps onto the next line, top-aligned. Use it for anything that
/// holds [`tile`]s: `horizontal_wrapped` centres each row and never wraps a
/// fixed-width frame, which is how cards ended up staggered and off-screen.
pub fn wrap<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    // Tiles look this up to find which row of which wrap they are in.
    let scope = WrapScope {
        id: ui.next_auto_id(),
        row: -1,
    };
    let outer = ui.data_mut(|data| {
        let outer = data.get_temp::<WrapScope>(wrap_scope_key());
        data.insert_temp(wrap_scope_key(), scope);
        outer
    });

    // Same starting size `horizontal_wrapped` uses: full width, one row tall. Asking
    // for the whole remaining area instead reserves it and pushes everything down.
    let size = egui::vec2(ui.available_size_before_wrap().x, ui.spacing().interact_size.y);
    let inner = ui
        .allocate_ui_with_layout(
            size,
            egui::Layout::left_to_right(egui::Align::TOP).with_main_wrap(true),
            add,
        )
        .inner;

    ui.data_mut(|data| match outer {
        Some(outer) => data.insert_temp(wrap_scope_key(), outer),
        None => data.remove::<WrapScope>(wrap_scope_key()),
    });
    inner
}

#[derive(Clone, Copy)]
struct WrapScope {
    id: egui::Id,
    /// Index of the row being filled; bumped by each tile that starts a row.
    row: i32,
}

fn wrap_scope_key() -> egui::Id {
    egui::Id::new("hexium-wrap-scope")
}

/// Tallest content seen in one row of a [`wrap`]: `current` is being measured this
/// frame, `previous` is last frame's result and is what tiles stretch to.
#[derive(Clone, Copy, Default)]
struct RowHeight {
    stamp: f64,
    current: f32,
    previous: f32,
}

/// A fixed-width card. Starts a new row first when it would not fit on this one,
/// and always stacks its own content vertically whatever the parent layout is.
pub fn tile<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    tile_inner(ui, width, add).0
}

/// A [`tile`] that is one big button: pointer cursor and an accent outline on
/// hover, and the outline stays while `selected`. Put no other buttons inside;
/// the card takes the click.
pub fn tile_button<R>(
    ui: &mut Ui,
    width: f32,
    selected: bool,
    add: impl FnOnce(&mut Ui) -> R,
) -> egui::Response {
    let (_, response) = tile_inner(ui, width, add);
    let response = response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if selected || response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            egui::Rounding::same(10.0),
            Stroke::new(if selected { 2.0 } else { 1.0 }, accent()),
        );
    }
    response
}

fn tile_inner<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> (R, egui::Response) {
    let frame = card_frame();
    let outer = width + frame.total_margin().sum().x;
    let row_started = ui.cursor().left() > ui.max_rect().left() + 1.0;
    if ui.layout().main_wrap && row_started && ui.available_size_before_wrap().x < outer {
        ui.end_row();
    }

    // Inside a `wrap`, every tile in a row grows to the tallest one. Immediate mode
    // cannot know that height up front, so it is measured one frame and applied the
    // next. Rows are counted rather than keyed by position: positions land on
    // half-pixels and round differently from one tile to the next.
    let starts_row = ui.cursor().left() <= ui.max_rect().left() + 1.0;
    let wrapping = ui.layout().main_wrap;
    let row = ui.data_mut(|data| {
        let mut scope = data.get_temp::<WrapScope>(wrap_scope_key())?;
        if !wrapping {
            return None;
        }
        if starts_row {
            scope.row += 1;
            data.insert_temp(wrap_scope_key(), scope);
        }
        Some(scope.id.with(scope.row))
    });

    let now = ui.input(|input| input.time);
    let mut settle = false;
    let target = match row {
        Some(key) => ui.data_mut(|data| {
            let entry = data.get_temp_mut_or_default::<RowHeight>(key);
            if entry.stamp != now {
                settle = (entry.current - entry.previous).abs() > 0.5;
                entry.previous = entry.current;
                entry.current = 0.0;
                entry.stamp = now;
            }
            entry.previous
        }),
        None => 0.0,
    };

    let shown = frame
        .show(ui, |ui| {
            ui.set_width(width);
            ui.vertical(|ui| {
                // `set_min_height` reserves space from the cursor down, so it has to
                // run before the content; called afterwards it adds the row height
                // on top of what is already there.
                let top = ui.cursor().top();
                if target > 0.0 {
                    ui.set_min_height(target);
                }
                let inner = add(ui);
                let natural =
                    (ui.cursor().top() - top - ui.spacing().item_spacing.y).max(0.0);
                if let Some(key) = row {
                    ui.data_mut(|data| {
                        let entry = data.get_temp_mut_or_default::<RowHeight>(key);
                        entry.current = entry.current.max(natural);
                        settle |= entry.current > entry.previous + 0.5;
                    });
                }
                inner
            })
            .inner
        });
    if settle {
        ui.ctx().request_repaint();
    }
    (shown.inner, shown.response)
}

/// A figure with its label: label in spaced capitals, value in the display face.
pub fn stat(ui: &mut Ui, label: &str, value: &str, color: Color32) {
    card_frame().show(ui, |ui| {
        ui.set_min_width(128.0);
        ui.vertical(|ui| {
            eyebrow(ui, label);
            ui.label(RichText::new(value).font(display(26.0)).color(color));
        });
    });
}

/// A message strip with a coloured bar down its left edge.
pub fn banner(ui: &mut Ui, text: &str, color: Color32) {
    callout(ui, color, |ui| {
        ui.label(RichText::new(text).size(13.0).color(INK));
    });
}

pub fn callout<R>(ui: &mut Ui, color: Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let shown = egui::Frame::none()
        .fill(card())
        .stroke(Stroke::new(1.0, line()))
        .rounding(egui::Rounding::same(10.0))
        .inner_margin(egui::Margin {
            left: 16.0,
            right: 12.0,
            top: 9.0,
            bottom: 9.0,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    let rect = shown.response.rect;
    let bar = egui::Rect::from_min_max(
        rect.left_top() + egui::vec2(1.0, 1.0),
        egui::pos2(rect.left() + 4.0, rect.bottom() - 1.0),
    );
    ui.painter().rect_filled(
        bar,
        egui::Rounding {
            nw: 9.0,
            sw: 9.0,
            ne: 0.0,
            se: 0.0,
        },
        color,
    );
    shown.inner
}

pub fn money(value: i64) -> String {
    let negative = value < 0;
    let digits = value.unsigned_abs().to_string();
    let mut out = String::new();
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(character);
    }
    if negative {
        format!("-{out}")
    } else {
        out
    }
}

pub fn compact(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

pub fn first_ten(text: &str) -> String {
    text.chars().take(10).collect()
}

/// Painted: online is a filled dot, offline an empty ring.
pub fn presence_dot(ui: &mut Ui, online: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, 12.0), Sense::hover());
    if online {
        ui.painter().circle_filled(rect.center(), 3.5, GREEN);
    } else {
        ui.painter()
            .circle_stroke(rect.center(), 3.0, Stroke::new(1.2, FAINT));
    }
}

// ---------------------------------------------------------------------------
// Shell
// ---------------------------------------------------------------------------

pub fn chrome(app: &mut App, cx: &Ctx, ctx: &egui::Context) {
    top_bar(app, cx, ctx);
    side_nav(app, ctx);
    // Hidden while a confirmation is up, so the dialog is never behind it.
    if app.pending_write.is_none() {
        messages::compose_window(app, ctx);
    }
    confirm_dialog(app, ctx);

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(bg()))
        .show(ctx, |ui| {
            let width = ui.available_width();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(22.0);
                    ui.horizontal_top(|ui| {
                        ui.add_space(28.0);
                        ui.vertical(|ui| {
                            ui.set_max_width((width - 56.0).max(320.0));
                            notice(app, ui);
                            match app.view {
                                View::Home => home::show(app, cx, ui),
                                View::Games => games::show(app, cx, ui),
                                View::Catalog => catalog::show(app, cx, ui),
                                View::Avatar => avatar::show(app, cx, ui),
                                View::Profile => profile::show(app, cx, ui),
                                View::Friends => friends::show(app, cx, ui),
                                View::Groups => groups::show(app, cx, ui),
                                View::Inventory => inventory::show(app, cx, ui),
                                View::Money => money::show(app, cx, ui),
                                View::Messages => messages::show(app, cx, ui),
                                View::Trades => trades::show(app, cx, ui),
                                View::Settings => settings::show(app, cx, ui),
                            }
                            ui.add_space(36.0);
                        });
                    });
                });
        });
}

/// Moves to a screen the same way the side menu does.
pub fn go(app: &mut App, view: View) {
    if view == View::Profile {
        app.show_own_profile();
    }
    if app.view != view || view == View::Profile {
        app.view = view;
        app.on_view_enter();
    }
}

fn brand(ui: &mut Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    let painter = ui.painter();
    hexagon(painter, rect.center(), size * 0.5, Some(accent()), None);
    hexagon(painter, rect.center(), size * 0.22, Some(panel()), None);
}

fn top_bar(app: &mut App, cx: &Ctx, ctx: &egui::Context) {
    egui::TopBottomPanel::top("top_bar")
        .exact_height(58.0)
        .frame(
            egui::Frame::none()
                .fill(panel())
                .stroke(Stroke::new(1.0, line()))
                .inner_margin(egui::Margin::symmetric(18.0, 0.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                brand(ui, 24.0);
                ui.label(RichText::new("Hexium").font(display(21.0)).color(INK));

                ui.add_space(18.0);
                if let Some(session) = app.session.ready() {
                    balance_chip(ui, "R$", &money(session.currency.robux), GOLD);
                    balance_chip(ui, "TIX", &money(session.currency.tix), GREEN);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(session) = app.session.ready() {
                        let headshot = session.headshot.clone();
                        let display_name = session.user.display_name.clone();
                        let response = egui::Frame::none()
                            .fill(card())
                            .stroke(Stroke::new(1.0, line()))
                            .rounding(egui::Rounding::same(20.0))
                            .inner_margin(egui::Margin {
                                left: 4.0,
                                right: 12.0,
                                top: 3.0,
                                bottom: 3.0,
                            })
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    app.thumbs.show(ui, cx, headshot.as_deref(), egui::vec2(28.0, 28.0));
                                    ui.label(RichText::new(display_name).font(semibold(13.0)));
                                });
                            })
                            .response
                            .interact(Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Your profile");
                        if response.hovered() {
                            ui.painter().rect_stroke(
                                response.rect,
                                egui::Rounding::same(20.0),
                                Stroke::new(1.0, accent()),
                            );
                        }
                        if response.clicked() {
                            go(app, View::Profile);
                        }
                    }
                    if ui
                        .add(egui::Button::new(RichText::new("Refresh").size(12.5)).fill(Color32::TRANSPARENT))
                        .on_hover_text("Reload this screen (F5)")
                        .clicked()
                    {
                        app.session = Load::Idle;
                        app.load_session();
                        app.reload_current_view();
                    }
                    if let Some(error) = &app.stale_error {
                        ui.label(RichText::new(error).size(11.5).color(DANGER));
                    }
                });
            });
        });
}

fn balance_chip(ui: &mut Ui, unit: &str, value: &str, color: Color32) {
    egui::Frame::none()
        .fill(tint(color, 26))
        .rounding(egui::Rounding::same(20.0))
        .inner_margin(egui::Margin::symmetric(11.0, 4.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(RichText::new(unit).font(semibold(10.5)).color(color));
                ui.label(RichText::new(value).font(display(16.0)).color(INK));
            });
        });
}

/// The side menu, grouped the way people think about the site rather than the
/// API. Counts from the session show as badges.
fn side_nav(app: &mut App, ctx: &egui::Context) {
    let counts = app.session.ready().map(|session| session.counts);
    let badge = |view: View| -> u64 {
        match (view, counts) {
            (View::Messages, Some(counts)) => counts.unread,
            (View::Friends, Some(counts)) => counts.friend_requests,
            (View::Trades, Some(counts)) => counts.trades,
            _ => 0,
        }
    };
    const GROUPS: [(&str, &[View]); 5] = [
        ("", &[View::Home]),
        ("You", &[View::Profile, View::Avatar, View::Inventory, View::Money]),
        ("Play", &[View::Games, View::Catalog]),
        ("Social", &[View::Friends, View::Messages, View::Trades, View::Groups]),
        ("App", &[View::Settings]),
    ];

    egui::SidePanel::left("side_nav")
        .resizable(false)
        .exact_width(216.0)
        .frame(
            egui::Frame::none()
                .fill(panel())
                .stroke(Stroke::new(1.0, line()))
                .inner_margin(egui::Margin::symmetric(12.0, 14.0)),
        )
        .show(ctx, |ui| {
            let mut target: Option<View> = None;
            // Scrolls when the window is too short to show every entry.
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (group, views) in GROUPS {
                        if !group.is_empty() {
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                ui.add_space(10.0);
                                ui.label(eyebrow_job(group, FAINT));
                            });
                            ui.add_space(2.0);
                        }
                        for view in views {
                            if nav_item(ui, view.label(), app.view == *view, badge(*view)) {
                                target = Some(*view);
                            }
                        }
                    }
                });

            if let Some(view) = target {
                // The Profile entry always means your own profile, even when
                // another user's profile is the one on screen.
                let away_from_own = view == View::Profile && app.profile.user_id.is_some();
                if app.view != view || away_from_own {
                    go(app, view);
                }
            }
        });
}

fn nav_item(ui: &mut Ui, label: &str, selected: bool, badge: u64) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 34.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let painter = ui.painter();
    let rounding = egui::Rounding::same(8.0);
    if selected {
        painter.rect_filled(rect, rounding, tint(accent(), 34));
    } else if response.hovered() {
        painter.rect_filled(rect, rounding, card());
    }

    let bullet = egui::pos2(rect.left() + 18.0, rect.center().y);
    if selected {
        hexagon(painter, bullet, 5.0, Some(accent()), None);
    } else {
        hexagon(painter, bullet, 4.5, None, Some(Stroke::new(1.2, FAINT)));
    }
    painter.text(
        egui::pos2(rect.left() + 34.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        if selected { semibold(14.0) } else { FontId::new(14.0, FontFamily::Proportional) },
        if selected { INK } else { Color32::from_rgb(196, 200, 206) },
    );

    if badge > 0 {
        let text = if badge > 99 { "99+".to_string() } else { badge.to_string() };
        let galley = painter.layout_no_wrap(text, semibold(10.5), on_accent());
        let size = egui::vec2((galley.size().x + 12.0).max(20.0), 18.0);
        let badge_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 10.0 - size.x / 2.0, rect.center().y),
            size,
        );
        painter.rect_filled(badge_rect, egui::Rounding::same(9.0), accent());
        painter.galley(badge_rect.center() - galley.size() / 2.0, galley, on_accent());
    }
    response.clicked()
}

// ---------------------------------------------------------------------------
// Sign-in gate
// ---------------------------------------------------------------------------

pub fn login(app: &mut App, _cx: &Ctx, ctx: &egui::Context) {
    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(bg()))
        .show(ctx, |ui| {
            ui.add_space(72.0);
            ui.vertical_centered(|ui| {
                brand(ui, 44.0);
                ui.add_space(6.0);
                ui.label(RichText::new("Hexium").font(display(38.0)).color(INK));
                ui.label(RichText::new("a desktop client for hexium.wtf").size(13.0).color(MUTED));
                ui.add_space(26.0);
            });

            ui.horizontal_top(|ui| {
                let width = ui.available_width();
                ui.add_space((width - 560.0).max(12.0) / 2.0);
                ui.vertical(|ui| {
                    ui.set_max_width(560.0);
                    card_frame()
                        .inner_margin(egui::Margin::same(22.0))
                        .show(ui, |ui| {
                            ui.set_width(516.0);
                            ui.label(RichText::new("Sign in").font(display(22.0)));
                            ui.add_space(2.0);
                            muted(
                                ui,
                                "hexium.wtf signs you in with your Roblox session cookie, not a \
                                 password of its own. This client sends it to hexium.wtf and \
                                 nowhere else.",
                            );
                            ui.add_space(12.0);
                            for (step, text) in [
                                ("1", "Open hexium.wtf in your browser, signed in."),
                                ("2", "DevTools → Application → Cookies → hexium.wtf."),
                                ("3", "Copy the value of .ROBLOSECURITY and paste it below."),
                            ] {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(step).font(display(15.0)).color(accent()));
                                    ui.label(RichText::new(text).size(13.0));
                                });
                            }
                            ui.add_space(12.0);

                            let response = ui.add(
                                egui::TextEdit::singleline(&mut app.login_input)
                                    .password(true)
                                    .hint_text(".ROBLOSECURITY value")
                                    .margin(egui::Margin::symmetric(10.0, 8.0))
                                    .desired_width(f32::INFINITY),
                            );
                            let submitted = response.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter));
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if primary_button(ui, "Sign in").clicked() || submitted {
                                    app.sign_in();
                                }
                                if ui.button("Clear").clicked() {
                                    app.login_input.clear();
                                }
                                if let Some(session) = app.cfg.masked_session() {
                                    muted(ui, &session);
                                }
                            });

                            if let Some(error) = &app.auth_error {
                                ui.add_space(6.0);
                                failed(ui, error);
                            }
                            if matches!(app.session, Load::Busy) {
                                ui.add_space(6.0);
                                waiting(ui, "your session");
                            }
                        });

                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(format!(
                            "Stored in {} · override with HEXIUM_COOKIE",
                            crate::config::config_path().display()
                        ))
                        .size(11.0)
                        .color(FAINT),
                    );
                });
            });
        });
}

// ---------------------------------------------------------------------------
// Writes: one confirmation dialog for every action that changes the account,
// and a dismissable line reporting how the last one went.
// ---------------------------------------------------------------------------

fn confirm_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(pending) = app.pending_write.as_ref() else {
        return;
    };
    let title = pending.action.title();
    let description = pending.action.description();
    let label = pending.action.confirm_label();
    let sending = pending.sending;
    let error = pending.error.clone();

    // Dim and block the rest of the window while the dialog is up.
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new("write-dialog-shade"))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(160));
        });

    let mut confirm = false;
    let mut cancel = false;
    egui::Window::new(RichText::new(title).font(display(19.0)))
        .id(egui::Id::new("write-dialog"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .collapsible(false)
        .resizable(false)
        .title_bar(true)
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new(description).size(13.5));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 14.0), Sense::hover());
                hexagon(ui.painter(), rect.center(), 4.0, Some(accent()), None);
                ui.label(
                    RichText::new("This changes your real hexium.wtf account.")
                        .size(12.0)
                        .color(MUTED),
                );
            });
            if let Some(error) = &error {
                ui.add_space(4.0);
                failed(ui, error);
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if sending {
                    ui.add(egui::Spinner::new().size(14.0).color(accent()));
                    muted(ui, "Sending…");
                } else {
                    if primary_button(ui, label).clicked() {
                        confirm = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                }
            });
            if !sending && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
        });

    if confirm {
        app.confirm_write();
    } else if cancel {
        app.cancel_write();
    }
}

fn notice(app: &mut App, ui: &mut Ui) {
    let Some((ok, text)) = app.notice.clone() else {
        return;
    };
    let color = if ok { GREEN } else { DANGER };
    let mut dismiss = false;
    callout(ui, color, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(text).size(13.0).color(INK));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Dismiss").clicked() {
                    dismiss = true;
                }
            });
        });
    });
    ui.add_space(10.0);
    if dismiss {
        app.notice = None;
    }
}

/// Small helper so screens can build the follow action without importing it.
pub fn follow_action(user_id: u64, name: &str, follow: bool) -> WriteAction {
    WriteAction::Follow {
        user_id,
        name: name.to_string(),
        follow,
    }
}
