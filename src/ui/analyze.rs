//! The trade analyzer's pieces: Heximons value, demand and trend per item, a
//! small price-history line, and a verdict for a whole trade by value and by
//! RAP. Shared by the trade details, the trade builder and the catalog.

use eframe::egui::{self, Pos2, RichText, Sense, Stroke, Ui};

use crate::api::models::ResaleData;
use crate::app::Load;
use crate::ui::{self, accent, GOLD, GREEN, MUTED};
use crate::values::{ItemValue, Values};

/// "Value 5,500 · Normal · Stable" under an item; nothing when Heximons does not
/// track it.
pub fn value_line(ui: &mut Ui, values: &Values, asset_id: u64) {
    let Some(item) = values.get(asset_id) else { return };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if let Some(value) = item.staff_value {
            ui.label(
                RichText::new(format!("Value {}", ui::money(value as i64)))
                    .size(11.0)
                    .font(ui::semibold(11.0))
                    .color(GOLD),
            )
            .on_hover_text("Heximons staff value");
        } else {
            ui.label(RichText::new("No staff value").size(10.5).color(MUTED))
                .on_hover_text("Heximons has not valued this item; its value is its RAP.");
        }
        if let Some(demand) = item.demand_label() {
            ui.label(RichText::new(format!("· {demand}")).size(10.5).color(demand_color(item)));
        }
        if let Some(trend) = item.trend_label() {
            ui.label(RichText::new(format!("· {trend}")).size(10.5).color(MUTED));
        }
        if item.rare {
            ui::pill(ui, "RARE", accent());
        }
    });
}

pub fn demand_color(item: &ItemValue) -> egui::Color32 {
    match item.demand {
        Some(3) => GREEN,
        Some(2) => ui::INK,
        Some(1) => ui::GOLD,
        Some(0) => ui::DANGER,
        _ => MUTED,
    }
}

/// A small line of an item's average sale price per day.
pub fn sparkline(ui: &mut Ui, state: &Load<ResaleData>) {
    let size = egui::vec2(72.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let data = match state {
        Load::Ready(data) => data,
        Load::Busy | Load::Idle => {
            ui.painter().line_segment(
                [Pos2::new(rect.left(), rect.center().y), Pos2::new(rect.right(), rect.center().y)],
                Stroke::new(1.0, ui::line()),
            );
            return;
        }
        Load::Err(_) => return,
    };
    let points: Vec<_> = data.points.iter().filter(|point| point.price > 0).collect();
    if points.len() < 2 {
        ui.painter().text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            if points.is_empty() { "no sales" } else { "1 day" },
            egui::FontId::proportional(9.5),
            MUTED,
        );
        return;
    }
    let first = points[0].day;
    let last = points[points.len() - 1].day;
    let span = (last - first).max(1) as f32;
    let low = points.iter().map(|point| point.price).min().unwrap_or(0) as f32;
    let high = points.iter().map(|point| point.price).max().unwrap_or(1) as f32;
    let range = (high - low).max(1.0);
    let line: Vec<Pos2> = points
        .iter()
        .map(|point| {
            Pos2::new(
                rect.left() + (point.day - first) as f32 / span * rect.width(),
                rect.bottom() - 2.0 - (point.price as f32 - low) / range * (rect.height() - 4.0),
            )
        })
        .collect();
    let rising = points[points.len() - 1].price >= points[0].price;
    let color = if rising { GREEN } else { ui::DANGER };
    ui.painter().add(egui::Shape::line(line.clone(), Stroke::new(1.5, color)));
    if let Some(pointer) = response.hover_pos() {
        // The nearest day with sales.
        let index = line
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| (a.x - pointer.x).abs().total_cmp(&(b.x - pointer.x).abs()))
            .map(|(index, _)| index)
            .unwrap_or(0);
        let point = points[index];
        ui.painter().circle_filled(line[index], 2.5, color);
        let sold = if point.volume == 1 { "1 sale".to_string() } else { format!("{} sales", point.volume) };
        crate::ui::chart::hover_box(
            ui.ctx(),
            pointer,
            &crate::ui::chart::short_date(point.day),
            &[(format!("Average {} R$", ui::money(point.price as i64)), color), (sold, MUTED)],
        );
    }
}

/// Totals of one side of a trade.
#[derive(Debug, Clone, Copy, Default)]
pub struct Side {
    pub rap: u64,
    pub value: u64,
    pub robux: u64,
}

/// "+5,500 by value (+40%)" style verdict for you, then the same by RAP, and a
/// warning for low-demand items you would receive.
pub fn verdict(ui: &mut Ui, give: Side, get: Side, weak: &[String]) {
    let by = |gain: i64, base: u64| -> (String, egui::Color32) {
        let percent = if base > 0 { gain as f64 / base as f64 * 100.0 } else { 0.0 };
        let sign = if gain > 0 { "+" } else { "" };
        let color = match gain.cmp(&0) {
            std::cmp::Ordering::Greater => GREEN,
            std::cmp::Ordering::Less => ui::DANGER,
            std::cmp::Ordering::Equal => MUTED,
        };
        (format!("{sign}{} ({sign}{percent:.0}%)", ui::money(gain)), color)
    };
    let value_gain = (get.value + get.robux) as i64 - (give.value + give.robux) as i64;
    let rap_gain = (get.rap + get.robux) as i64 - (give.rap + give.robux) as i64;
    ui.horizontal_wrapped(|ui| {
        ui::eyebrow(ui, "For you");
        let (text, color) = by(value_gain, give.value + give.robux);
        ui.label(RichText::new(format!("{text} by value")).font(ui::semibold(13.0)).color(color))
            .on_hover_text("Heximons value where it has one, otherwise RAP. Robux counts at face value.");
        let (text, color) = by(rap_gain, give.rap + give.robux);
        ui.label(RichText::new(format!("· {text} by RAP")).size(12.0).color(color));
    });
    if !weak.is_empty() {
        ui.label(
            RichText::new(format!("Low demand on what you get: {}", weak.join(", ")))
                .size(11.5)
                .color(ui::GOLD),
        );
    }
}

/// Names of items with Low or Terrible demand.
pub fn weak_demand<'a>(values: &Values, assets: impl Iterator<Item = (u64, &'a str)>) -> Vec<String> {
    assets
        .filter(|(asset, _)| values.get(*asset).and_then(|item| item.demand).is_some_and(|demand| demand <= 1))
        .map(|(_, name)| name.to_string())
        .collect()
}

/// The combination of up to `limit` items (copy id, value) whose total comes
/// closest to `target`; ties go to fewer items. Looks at the 40 most valuable.
pub fn suggest(items: &[(u64, u64)], target: u64, limit: usize) -> Vec<u64> {
    let mut pool: Vec<(u64, u64)> = items.iter().copied().filter(|(_, value)| *value > 0).collect();
    pool.sort_by(|a, b| b.1.cmp(&a.1));
    pool.truncate(40);
    let mut best: (u64, usize, Vec<u64>) = (u64::MAX, usize::MAX, Vec::new());
    let n = pool.len();
    let mut consider = |picked: &[usize]| {
        let sum: u64 = picked.iter().map(|index| pool[*index].1).sum();
        let distance = sum.abs_diff(target);
        if (distance, picked.len()) < (best.0, best.1) {
            best = (distance, picked.len(), picked.iter().map(|index| pool[*index].0).collect());
        }
    };
    for a in 0..n {
        consider(&[a]);
        if limit < 2 { continue; }
        for b in a + 1..n {
            consider(&[a, b]);
            if limit < 3 { continue; }
            for c in b + 1..n {
                consider(&[a, b, c]);
                if limit < 4 { continue; }
                for d in c + 1..n {
                    consider(&[a, b, c, d]);
                }
            }
        }
    }
    best.2
}

#[cfg(test)]
mod suggest_tests {
    use super::suggest;

    #[test]
    fn closest_combination() {
        let items = [(1, 1000), (2, 600), (3, 450), (4, 300), (5, 50)];
        // Only 450 + 300 make exactly 750.
        let mut picked = suggest(&items, 750, 4);
        picked.sort();
        assert_eq!(picked, vec![3, 4]);
        // Only 1000 + 600 + 50 make exactly 1650.
        let mut picked = suggest(&items, 1650, 4);
        picked.sort();
        assert_eq!(picked, vec![1, 2, 5]);
        // At most two items: 1000 + 600 is closest to 1650.
        let mut picked = suggest(&items, 1650, 2);
        picked.sort();
        assert_eq!(picked, vec![1, 2]);
        assert_eq!(suggest(&items, 1000, 4), vec![1]);
        assert!(suggest(&[], 500, 4).is_empty());
    }
}
