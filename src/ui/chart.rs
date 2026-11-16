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
            painter.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], Stroke::new(1.0, ui::tint(ui::INK, 90)));
            if point.price > 0 {
                painter.circle_stroke(Pos2::new(x, y_of(point.price)), 4.5, Stroke::new(1.5, ui::INK));
            }
            let sold = if point.volume == 1 { "1 sale".to_string() } else { format!("{} sales", point.volume) };
            response.on_hover_text(format!(
                "{}\nAverage {} R$\n{sold}",
                short_date(point.day),
                ui::money(point.price as i64)
            ));
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
    fn short_dates_round_trip() {
        assert_eq!(short_date(day_number("2026-11-16").unwrap()), "Nov 16");
        assert_eq!(short_date(day_number("2026-07-27").unwrap()), "Jul 27");
        assert_eq!(short_date(day_number("2024-02-29").unwrap()), "Feb 29");
    }
}
