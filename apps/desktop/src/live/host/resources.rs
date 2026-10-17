//! Shared Host panel framing, resource tones, Kit charts and number formatting.
use super::*;
use gpui_kit::component::{
    chart::AreaChart,
    group_box::{GroupBox, GroupBoxVariants},
};

/// Semantic theme tokens per host resource. Kit's default chart palette is a
/// single hue, so resources map to distinct status and chart tokens instead.
#[derive(Clone, Copy)]
pub(super) struct Tones {
    pub cpu: Hsla,
    pub gpu: Hsla,
    pub memory: Hsla,
    pub cache: Hsla,
    pub swap: Hsla,
    pub download: Hsla,
    pub upload: Hsla,
    pub write: Hsla,
    pub read: Hsla,
    pub track: Hsla,
    warning: Hsla,
    danger: Hsla,
}

impl Tones {
    pub fn of(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            cpu: theme.success,
            gpu: theme.chart_2,
            memory: theme.warning,
            cache: theme.warning.opacity(0.5),
            swap: theme.muted_foreground.opacity(0.5),
            download: theme.chart_2,
            upload: theme.info,
            write: theme.warning,
            read: theme.success,
            track: theme.muted_foreground.opacity(0.16),
            warning: theme.warning,
            danger: theme.danger,
        }
    }

    /// Utilization switches to the warning tone from 80%.
    pub fn load(&self, base: Hsla, percent: f32) -> Hsla {
        if percent >= 80. { self.warning } else { base }
    }

    /// Capacity warns from 70% and turns to danger from 85%.
    pub fn capacity(&self, percent: f32) -> Hsla {
        if percent >= 85. {
            self.danger
        } else if percent >= 70. {
            self.warning
        } else {
            self.cpu
        }
    }
}

/// The bordered card surface shared by every Host panel section.
pub(super) fn panel(id: &'static str, cx: &App) -> GroupBox {
    GroupBox::new().id(id).fill().min_w_0().content_style(
        StyleRefinement::default()
            .flex_1()
            .min_w_0()
            .p_5()
            .gap_3p5()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_lg),
    )
}

/// Half-row cards share one row and wrap below each other on narrow widths.
pub(super) fn half(card: GroupBox) -> GroupBox {
    card.flex_1().flex_basis(px(380.))
}

pub(super) fn mono<E: Styled>(element: E, cx: &App) -> E {
    element.font_family(cx.theme().mono_font_family.clone())
}

pub(super) fn tick(tone: Hsla) -> Div {
    div()
        .w(px(3.))
        .h(px(12.))
        .flex_shrink_0()
        .rounded(px(2.))
        .bg(tone)
}

pub(super) fn title(label: &str) -> Div {
    div().text_sm().font_semibold().child(tr(label))
}

/// A muted resource label with its series tick.
pub(super) fn heading(label: &str, tone: Hsla, cx: &App) -> Div {
    h_flex()
        .gap_1p5()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(tick(tone))
        .child(tr(label))
}

/// The large monospace value with its unit.
pub(super) fn figure(value: String, unit: &str, cx: &App) -> Div {
    mono(
        h_flex()
            .items_baseline()
            .gap_1()
            .child(div().text_size(px(32.)).font_bold().child(value))
            .when(!unit.is_empty(), |figure| {
                figure.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(unit.to_string()),
                )
            }),
        cx,
    )
}

/// An inline legend entry: tick, label and the current monospace value.
pub(super) fn legend(label: &str, value: String, tone: Hsla, cx: &App) -> Div {
    h_flex()
        .gap_1p5()
        .whitespace_nowrap()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(tick(tone))
        .child(tr(label))
        .child(mono(
            div().text_color(cx.theme().foreground).child(value),
            cx,
        ))
}

/// A rate statistic with an optional cumulative total beside it.
pub(super) fn stat(label: &str, value: String, total: Option<String>, tone: Hsla, cx: &App) -> Div {
    v_flex()
        .min_w_0()
        .gap_1()
        .child(heading(label, tone, cx))
        .child(
            h_flex()
                .items_baseline()
                .gap_2()
                .min_w_0()
                .child(mono(div().text_lg().whitespace_nowrap().child(value), cx))
                .when_some(total, |row, total| {
                    row.child(mono(
                        div()
                            .text_xs()
                            .whitespace_nowrap()
                            .text_color(cx.theme().muted_foreground)
                            .child(total),
                        cx,
                    ))
                }),
        )
}

/// Placeholder shown until a chart has two consecutive samples.
pub(super) fn pending(points: usize, cx: &App) -> Div {
    h_flex()
        .size_full()
        .justify_center()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(tr(if points == 0 {
            "host_metric_unavailable"
        } else {
            "host_sampling"
        }))
}

/// Two byte-rate series over the last minute with rate axis labels.
pub(super) fn transfer_chart(
    id: &'static str,
    points: Vec<(String, f64, f64)>,
    first: (Hsla, SharedString),
    second: (Hsla, SharedString),
    cx: &App,
) -> Div {
    let body = div()
        .h(px(96.))
        .w_full()
        .min_w_0()
        .debug_selector(move || id.into());
    if points.len() < 2 {
        return body.child(pending(points.len(), cx));
    }
    // A floor keeps idle traffic from stretching noise across the full height.
    let peak = points
        .iter()
        .flat_map(|point| [point.1, point.2])
        .fold(1024., f64::max);
    body.child(
        AreaChart::new(points)
            .id(id)
            .x(|point| point.0.clone())
            .y(|point| point.1)
            .stroke(first.0)
            .fill(first.0.opacity(0.14))
            .name(first.1)
            .linear()
            .y(|point| point.2)
            .stroke(second.0)
            .fill(gpui_kit::transparent_black())
            .name(second.1)
            .linear()
            .y_domain(0., peak)
            .y_axis(true)
            .y_tick_count(3)
            .y_tick_format(|value| rate(value.max(0.) as u64))
            .tooltip_title(|point| point.0.clone().into())
            .tooltip_value(|_, _, value| rate(value.max(0.) as u64).into())
            .x_axis(false)
            .grid_dashed(true),
    )
}

pub(super) fn series(
    points: impl Iterator<Item = (u64, Option<(f64, f64)>)>,
) -> Vec<(String, f64, f64)> {
    let mut values = Vec::new();
    for (time, point) in points {
        if let Some((value, second)) = point {
            values.push((clock(time), value, second));
        } else {
            values.clear();
        }
    }
    values
}

pub(super) fn clock(time: u64) -> String {
    chrono::DateTime::from_timestamp_millis(time as i64)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}

pub(super) fn percent(value: Option<u32>) -> String {
    value
        .map(|value| format!("{:.1}%", value as f64 / 100.))
        .unwrap_or_else(|| "—".into())
}

/// A byte count split into its number and binary unit.
pub(super) fn scaled(value: u64) -> (String, &'static str) {
    let (scale, unit) = if value >= 1_073_741_824 {
        (1_073_741_824., "GiB")
    } else if value >= 1_048_576 {
        (1_048_576., "MiB")
    } else if value >= 1024 {
        (1024., "KiB")
    } else {
        (1., "B")
    };
    (format!("{:.1}", value as f64 / scale), unit)
}

pub(super) fn bytes(value: u64) -> String {
    let (value, unit) = scaled(value);
    format!("{value} {unit}")
}

pub(super) fn rate(value: u64) -> String {
    format!("{}/s", bytes(value))
}

#[cfg(test)]
mod tests {
    use super::{bytes, scaled};
    use core::prelude::v1::test;

    #[test]
    fn splits_binary_units() {
        assert_eq!(scaled(512), ("512.0".into(), "B"));
        assert_eq!(scaled(1536), ("1.5".into(), "KiB"));
        assert_eq!(scaled(3 * 1_073_741_824), ("3.0".into(), "GiB"));
        assert_eq!(bytes(1_048_576), "1.0 MiB");
    }
}
