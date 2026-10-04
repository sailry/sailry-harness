//! Signed transfer series compose Kit's native chart and hover behavior.
use super::*;
use gpui_kit::component::chart::AreaChart;

#[cfg(test)]
mod tests;

pub(super) fn chart(
    id: &'static str,
    points: Vec<(String, f64, f64)>,
    lower: (Hsla, SharedString),
    upper: (Hsla, SharedString),
) -> AreaChart<(String, f64, f64), String, f64> {
    let peak = points
        .iter()
        .flat_map(|point| [point.1, point.2])
        .fold(0., f64::max)
        .max(f64::EPSILON);
    AreaChart::new(points)
        .id(id)
        .x(|point| point.0.clone())
        .y(|point| point.2)
        .stroke(upper.0)
        .fill(upper.0.opacity(0.16))
        .name(upper.1)
        .linear()
        .y(|point| -point.1)
        .stroke(lower.0)
        .fill(lower.0.opacity(0.16))
        .name(lower.1)
        .linear()
        .y_domain(-peak, peak)
        .y_padding(4., 4.)
        .baseline(0.)
        .reference_line(0.)
        .tooltip_title(|point| point.0.clone().into())
        .tooltip_value(|_, _, value| value_label(value))
        .x_axis(false)
        .grid(false)
}

fn value_label(value: f64) -> SharedString {
    super::resources::rounded(value.abs()).to_string().into()
}
