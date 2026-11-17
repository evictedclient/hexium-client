//! The RAP and sales history chart for a limited: average sale price per day as
//! a line, sales per day as bars along the bottom, and RAP as a reference line.
//! Drawn with the painter in flat colours; x is real dates, so quiet stretches
//! show as gaps.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui};

use crate::api::models::{ResaleData, ResalePoint};
use crate::ui::{self, accent, muted, GOLD, MUTED};

const HEIGHT: f32 = 170.0;
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `Nov 16` for a day number.
pub fn short_date(day: i64) -> String {
    // Howard Hinnant's civil_from_days.
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day_of_month = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{} {day_of_month}", MONTHS[(month - 1) as usize])
}

/// Seconds to add to UTC for local time (Windows' time-zone setting, daylight
/// saving included); read once.
pub fn local_offset_secs() -> i64 {
    static OFFSET: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *OFFSET.get_or_init(read_offset)
}

#[cfg(windows)]
fn read_offset() -> i64 {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    let mut info = TIME_ZONE_INFORMATION::default();
    // 2 = TIME_ZONE_ID_DAYLIGHT: daylight saving is in effect now.
    let kind = unsafe { GetTimeZoneInformation(&mut info) };
    let bias = info.Bias + if kind == 2 { info.DaylightBias } else { info.StandardBias };
    -(bias as i64) * 60
}

#[cfg(not(windows))]
fn read_offset() -> i64 {
    0
}

/// "Nov 16, 18:04" in local time, for a moment in seconds since 1970 (UTC).
pub fn local_time(secs: u64) -> String {
    let local = secs as i64 + local_offset_secs();
    let day = local.div_euclid(86_400);
    let of_day = local.rem_euclid(86_400);
    format!("{}, {:02}:{:02}", short_date(day), of_day / 3600, (of_day % 3600) / 60)
}

/// The box shown while hovering a chart: a heading (the time) and coloured
/// lines (the values), next to the pointer. Drawn on the tooltip layer so it
/// appears at once, sits above everything and is never cut off by the chart;
/// it flips to the other side of the pointer rather than leave the window.
pub fn hover_box(ctx: &egui::Context, pointer: Pos2, heading: &str, lines: &[(String, Color32)]) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("chart-hover-box")));
    let heading_font = FontId::proportional(11.0);
    let line_font = FontId::proportional(12.5);
    let heading_galley = painter.layout_no_wrap(heading.to_string(), heading_font, MUTED);
    let galleys: Vec<_> = lines
        .iter()
        .map(|(text, color)| painter.layout_no_wrap(text.clone(), line_font.clone(), *color))
        .collect();
    let pad = egui::vec2(9.0, 6.0);
    let gap = 2.0;
    let width = galleys
        .iter()
        .map(|galley| galley.size().x)
        .fold(heading_galley.size().x, f32::max)
        + pad.x * 2.0;
    let height = heading_galley.size().y
        + galleys.iter().map(|galley| galley.size().y + gap).sum::<f32>()
        + pad.y * 2.0;
    let screen = ctx.screen_rect();
    let mut min = pointer + egui::vec2(14.0, -height - 10.0);
    if min.x + width > screen.right() - 4.0 {
        min.x = pointer.x - width - 14.0;
    }
    if min.y < screen.top() + 4.0 {
        min.y = pointer.y + 16.0;
    }
    min.x = min.x.clamp(screen.left() + 4.0, (screen.right() - width - 4.0).max(screen.left() + 4.0));
    let rect = Rect::from_min_size(min, egui::vec2(width, height));
    painter.rect_filled(rect, egui::Rounding::same(7.0), ui::raised());
    painter.rect_stroke(rect, egui::Rounding::same(7.0), Stroke::new(1.0, ui::line()));
    let mut cursor = rect.min + pad;
    painter.galley(cursor, heading_galley.clone(), MUTED);
    cursor.y += heading_galley.size().y + gap;
    for galley in galleys {
        let height = galley.size().y;
        painter.galley(cursor, galley, ui::INK);
        cursor.y += height + gap;
    }
}

/// A guide line through the hovered point, and a ring on each marked value.
fn hover_marks(painter: &egui::Painter, plot: Rect, x: f32, marks: &[(f32, Color32)]) {
    painter.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], Stroke::new(1.0, ui::tint(ui::INK, 90)));
    for (y, color) in marks {
        painter.circle_filled(Pos2::new(x, *y), 4.0, ui::card());
        painter.circle_stroke(Pos2::new(x, *y), 4.0, Stroke::new(2.0, *color));
    }
}

pub fn summary(ui: &mut Ui, data: &ResaleData) {
    ui.horizontal_wrapped(|ui| {
        stat(ui, "RAP", &format!("{} R$", ui::money(data.rap as i64)), GOLD);
        stat(ui, "Sales", &ui::compact(data.sales), ui::INK);
        let copies = data.total_copies.max(data.stock);
        if copies > 0 {
            stat(ui, "Copies", &ui::compact(copies), ui::INK);
        }
        if data.remaining > 0 {
            stat(ui, "Left from creator", &ui::compact(data.remaining), ui::INK);
        }
    });
}

fn stat(ui: &mut Ui, label: &str, value: &str, color: Color32) {
    ui.vertical(|ui| {
        ui::eyebrow(ui, label);
        ui.label(RichText::new(value).font(ui::semibold(14.0)).color(color));
    });
    ui.add_space(14.0);
}

pub fn show(ui: &mut Ui, data: &ResaleData) {
    show_with_price(ui, data, None);
}

/// The chart with a price you are considering drawn across it.
pub fn show_with_price(ui: &mut Ui, data: &ResaleData, yours: Option<u64>) {
    if data.points.is_empty() {
        muted(ui, "No sales recorded yet.");
        return;
    }
    let width = ui.available_width().max(260.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, HEIGHT), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, egui::Rounding::same(8.0), ui::card());
    painter.rect_stroke(rect, egui::Rounding::same(8.0), Stroke::new(1.0, ui::line()));

    // Plot area: room for the price labels on the left and dates underneath.
    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + 46.0, rect.top() + 12.0),
        Pos2::new(rect.right() - 12.0, rect.bottom() - 22.0),
    );
    let bars_height = plot.height() * 0.28;

    let first = data.points.first().map(|point| point.day).unwrap_or(0);
    let last = data.points.last().map(|point| point.day).unwrap_or(first);
    let span = (last - first).max(1) as f32;
    let x_of = |day: i64| {
        if last == first {
            plot.center().x
        } else {
            plot.left() + (day - first) as f32 / span * plot.width()
        }
    };
    let top_price = data
        .points
        .iter()
        .map(|point| point.price)
        .chain(std::iter::once(data.rap))
        .chain(yours)
        .max()
        .unwrap_or(1)
        .max(1) as f32
        * 1.1;
    let y_of = |price: u64| plot.bottom() - price as f32 / top_price * plot.height();
    let top_volume = data.points.iter().map(|point| point.volume).max().unwrap_or(1).max(1) as f32;

    let small = FontId::proportional(10.0);
    // Scale: top and middle price.
    for fraction in [1.0_f32, 0.5] {
        let price = (top_price / 1.1 * fraction) as u64;
        let y = y_of(price);
        painter.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0, ui::line()));
        painter.text(Pos2::new(plot.left() - 6.0, y), Align2::RIGHT_CENTER, ui::compact(price), small.clone(), MUTED);
    }

    // Sales per day.
    let bar_width = (plot.width() / (span + 1.0)).clamp(2.0, 10.0);
    for point in &data.points {
        let height = point.volume as f32 / top_volume * bars_height;
        let x = x_of(point.day);
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(x - bar_width / 2.0, plot.bottom() - height), Pos2::new(x + bar_width / 2.0, plot.bottom())),
            egui::Rounding::same(1.5),
            ui::tint(MUTED, 110),
        );
    }

    // RAP reference.
    if data.rap > 0 {
        let y = y_of(data.rap);
        painter.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0, ui::tint(GOLD, 170)));
        painter.text(Pos2::new(plot.right(), y - 2.0), Align2::RIGHT_BOTTOM, format!("RAP {}", ui::compact(data.rap)), small.clone(), GOLD);
    }

    // The price being considered.
    if let Some(price) = yours.filter(|price| *price > 0) {
        let y = y_of(price);
        let color = ui::GREEN;
        // Short dashes, so it reads as "not a sale".
        let mut x = plot.left();
        while x < plot.right() {
            let end = (x + 6.0).min(plot.right());
            painter.line_segment([Pos2::new(x, y), Pos2::new(end, y)], Stroke::new(1.5, color));
            x += 10.0;
        }
        painter.text(
            Pos2::new(plot.left() + 4.0, y - 2.0),
            Align2::LEFT_BOTTOM,
            format!("Your price {}", ui::compact(price)),
            small.clone(),
            color,
        );
    }

    // Average sale price per day.
    let line: Vec<Pos2> = data
        .points
        .iter()
        .filter(|point| point.price > 0)
        .map(|point| Pos2::new(x_of(point.day), y_of(point.price)))
        .collect();
    if line.len() > 1 {
        painter.add(egui::Shape::line(line.clone(), Stroke::new(2.0, accent())));
    }
    for dot in &line {
        painter.circle_filled(*dot, 2.5, accent());
    }

    // Dates at both ends.
    painter.text(Pos2::new(plot.left(), rect.bottom() - 6.0), Align2::LEFT_BOTTOM, short_date(first), small.clone(), MUTED);
    if last != first {
        painter.text(Pos2::new(plot.right(), rect.bottom() - 6.0), Align2::RIGHT_BOTTOM, short_date(last), small, MUTED);
    }

    // Hover: the nearest day.
    if let Some(pointer) = response.hover_pos() {
        if let Some(point) = nearest(&data.points, |day| x_of(day), pointer.x) {
            let x = x_of(point.day);
            let marks: Vec<(f32, Color32)> =
                if point.price > 0 { vec![(y_of(point.price), accent())] } else { Vec::new() };
            hover_marks(&painter, plot, x, &marks);
            let sold = if point.volume == 1 { "1 sale".to_string() } else { format!("{} sales", point.volume) };
            let mut lines = vec![
                (format!("Average {} R$", ui::money(point.price as i64)), accent()),
                (sold, MUTED),
            ];
            if data.rap > 0 {
                lines.push((format!("RAP now {} R$", ui::money(data.rap as i64)), GOLD));
            }
            hover_box(ui.ctx(), pointer, &short_date(point.day), &lines);
        }
    }
}

fn nearest(points: &[ResalePoint], x_of: impl Fn(i64) -> f32, x: f32) -> Option<ResalePoint> {
    points
        .iter()
        .min_by(|a, b| (x_of(a.day) - x).abs().total_cmp(&(x_of(b.day) - x).abs()))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::day_number;

    #[test]
    fn local_time_formats() {
        // The offset is whatever this machine uses; check the shape and that a
        // whole day of offset never breaks the date maths.
        let text = local_time(1_794_210_574);
        assert!(text.contains(", ") && text.len() >= 11, "{text}");
        let (_, clock) = text.split_once(", ").unwrap();
        assert_eq!(clock.len(), 5);
        assert_eq!(&clock[2..3], ":");
    }

    #[test]
    fn short_dates_round_trip() {
        assert_eq!(short_date(day_number("2026-11-16").unwrap()), "Nov 16");
        assert_eq!(short_date(day_number("2026-07-27").unwrap()), "Jul 27");
        assert_eq!(short_date(day_number("2024-02-29").unwrap()), "Feb 29");
    }
}

/// Your collectibles' total per day: value (gold) and RAP (accent).
pub fn history(ui: &mut Ui, points: &[crate::values::HistoryPoint]) {
    if points.is_empty() {
        return;
    }
    let width = ui.available_width().max(260.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 140.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, egui::Rounding::same(8.0), ui::card());
    painter.rect_stroke(rect, egui::Rounding::same(8.0), Stroke::new(1.0, ui::line()));
    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + 52.0, rect.top() + 12.0),
        Pos2::new(rect.right() - 12.0, rect.bottom() - 22.0),
    );
    let first = points[0].day;
    let last = points[points.len() - 1].day;
    let span = (last - first).max(1) as f32;
    let x_of = |day: i64| {
        if first == last {
            plot.center().x
        } else {
            plot.left() + (day - first) as f32 / span * plot.width()
        }
    };
    let top = points.iter().map(|point| point.value.max(point.rap)).max().unwrap_or(1).max(1) as f32 * 1.1;
    let y_of = |amount: u64| plot.bottom() - amount as f32 / top * plot.height();
    let small = FontId::proportional(10.0);
    for fraction in [1.0_f32, 0.5] {
        let amount = (top / 1.1 * fraction) as u64;
        let y = y_of(amount);
        painter.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0, ui::line()));
        painter.text(Pos2::new(plot.left() - 6.0, y), Align2::RIGHT_CENTER, ui::compact(amount), small.clone(), MUTED);
    }
    for (pick, color) in [(0usize, GOLD), (1usize, accent())] {
        let line: Vec<Pos2> = points
            .iter()
            .map(|point| Pos2::new(x_of(point.day), y_of(if pick == 0 { point.value } else { point.rap })))
            .collect();
        if line.len() > 1 {
            painter.add(egui::Shape::line(line.clone(), Stroke::new(2.0, color)));
        }
        for dot in &line {
            painter.circle_filled(*dot, 3.0, color);
        }
    }
    painter.text(Pos2::new(plot.left(), rect.bottom() - 6.0), Align2::LEFT_BOTTOM, short_date(first), small.clone(), MUTED);
    if last != first {
        painter.text(Pos2::new(plot.right(), rect.bottom() - 6.0), Align2::RIGHT_BOTTOM, short_date(last), small, MUTED);
    }
    if let Some(pointer) = response.hover_pos() {
        let nearest = points
            .iter()
            .min_by(|a, b| (x_of(a.day) - pointer.x).abs().total_cmp(&(x_of(b.day) - pointer.x).abs()))
            .copied();
        if let Some(point) = nearest {
            hover_marks(&painter, plot, x_of(point.day), &[(y_of(point.value), GOLD), (y_of(point.rap), accent())]);
            hover_box(
                ui.ctx(),
                pointer,
                &short_date(point.day),
                &[
                    (format!("Value {}", ui::money(point.value as i64)), GOLD),
                    (format!("RAP {}", ui::money(point.rap as i64)), accent()),
                ],
            );
        }
    }
}


/// A player's value (gold) and RAP (accent) over time, from Heximons. Points are
/// taken every few minutes, so a long history is thinned to one per pixel.
pub fn player_history(ui: &mut Ui, points: &[crate::values::PlayerPoint], height: f32) {
    if points.len() < 2 {
        muted(ui, "Not enough history yet.");
        return;
    }
    let width = ui.available_width().max(260.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, egui::Rounding::same(8.0), ui::card());
    painter.rect_stroke(rect, egui::Rounding::same(8.0), Stroke::new(1.0, ui::line()));
    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + 52.0, rect.top() + 12.0),
        Pos2::new(rect.right() - 12.0, rect.bottom() - 22.0),
    );
    let first = points[0].time;
    let last = points[points.len() - 1].time;
    let span = (last - first).max(1) as f32;
    let x_of = |time: u64| plot.left() + (time - first) as f32 / span * plot.width();
    let top = points.iter().map(|point| point.value.max(point.rap)).max().unwrap_or(1).max(1) as f32 * 1.1;
    let y_of = |amount: u64| plot.bottom() - amount as f32 / top * plot.height();
    let small = FontId::proportional(10.0);
    for fraction in [1.0_f32, 0.5] {
        let amount = (top / 1.1 * fraction) as u64;
        let y = y_of(amount);
        painter.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0, ui::line()));
        painter.text(Pos2::new(plot.left() - 6.0, y), Align2::RIGHT_CENTER, ui::compact(amount), small.clone(), MUTED);
    }
    // One point per pixel column is plenty.
    let step = (points.len() as f32 / plot.width()).ceil().max(1.0) as usize;
    let thinned: Vec<&crate::values::PlayerPoint> = points.iter().step_by(step).chain(points.last()).collect();
    for (rap, color) in [(true, accent()), (false, GOLD)] {
        let line: Vec<Pos2> = thinned
            .iter()
            .map(|point| Pos2::new(x_of(point.time), y_of(if rap { point.rap } else { point.value })))
            .collect();
        painter.add(egui::Shape::line(line, Stroke::new(1.8, color)));
    }
    let day = |time: u64| short_date((time / 86_400) as i64);
    painter.text(Pos2::new(plot.left(), rect.bottom() - 6.0), Align2::LEFT_BOTTOM, day(first), small.clone(), MUTED);
    painter.text(Pos2::new(plot.right(), rect.bottom() - 6.0), Align2::RIGHT_BOTTOM, day(last), small, MUTED);
    if let Some(pointer) = response.hover_pos() {
        let target = first + ((pointer.x - plot.left()).max(0.0) / plot.width() * span) as u64;
        let index = points.partition_point(|point| point.time < target).min(points.len() - 1);
        // The nearer of the two points around the pointer.
        let index = if index > 0 && target.abs_diff(points[index - 1].time) < points[index].time.abs_diff(target) {
            index - 1
        } else {
            index
        };
        let point = points[index];
        hover_marks(&painter, plot, x_of(point.time), &[(y_of(point.value), GOLD), (y_of(point.rap), accent())]);
        hover_box(
            ui.ctx(),
            pointer,
            &local_time(point.time),
            &[
                (format!("Value {}", ui::money(point.value as i64)), GOLD),
                (format!("RAP {}", ui::money(point.rap as i64)), accent()),
                (format!("{} collectibles", point.collectibles), MUTED),
            ],
        );
    }
}
