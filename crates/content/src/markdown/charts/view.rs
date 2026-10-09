use super::{Chart, Data};
use crate::markdown::style::Theme;
use gpui_kit::component::{
    chart::{AreaChart, BarChart, CandlestickChart, LineChart, PieChart, RadarChart, SankeyChart},
    plot::shape::SankeyLink,
};
use gpui_kit::{AnyElement, IntoElement, SharedString, div, prelude::*, px};

pub(in crate::markdown) fn render(source: &str, block: usize, theme: &Theme) -> Option<AnyElement> {
    let chart = Chart::parse(source)?;
    let id = format!("markdown-chart-{block}");
    let title = chart.title.clone();
    let description = chart.description();
    let element = match chart.data {
        Data::Line { data } => LineChart::new(data)
            .id(id)
            .x(|point| SharedString::from(point.label.clone()))
            .y(|point| point.value)
            .stroke(theme.accent)
            .into_any_element(),
        Data::Bar { data } => {
            let accent = theme.accent;
            BarChart::new(data)
                .id(id)
                .band(|point| SharedString::from(point.label.clone()))
                .value(|point| point.value)
                .fill(move |_, _, _, _| accent)
                .into_any_element()
        }
        Data::Area { data } => AreaChart::new(data)
            .id(id)
            .x(|point| SharedString::from(point.label.clone()))
            .y(|point| point.value)
            .stroke(theme.accent)
            .fill(theme.accent.opacity(0.18))
            .into_any_element(),
        Data::Pie { data } => {
            let colors = [
                theme.accent,
                theme.success,
                theme.warning,
                theme.danger,
                theme.busy,
            ];
            let colors: std::collections::HashMap<_, _> = data
                .iter()
                .enumerate()
                .map(|(index, point)| (point.label.clone(), colors[index % colors.len()]))
                .collect();
            PieChart::new(data)
                .id(id)
                .value(|point| point.value as f32)
                .label(|point| point.label.clone().into())
                .color(move |point| colors[&point.label])
                .label_color(theme.text)
                .into_any_element()
        }
        Data::Radar { data } => RadarChart::new(data)
            .id(id)
            .value(|point| point.value)
            .label(|point| point.label.clone())
            .stroke(theme.accent)
            .fill(theme.accent.opacity(0.18))
            .label_color(theme.text)
            .into_any_element(),
        Data::Candlestick { data } => CandlestickChart::new(data)
            .id(id)
            .x(|point| SharedString::from(point.label.clone()))
            .open(|point| point.open)
            .high(|point| point.high)
            .low(|point| point.low)
            .close(|point| point.close)
            .bullish(theme.success)
            .bearish(theme.danger)
            .into_any_element(),
        Data::Sankey { nodes, links } => {
            let ids: std::collections::HashMap<_, _> = nodes
                .iter()
                .enumerate()
                .map(|(index, node)| (node.id.clone(), index))
                .collect();
            let links = links
                .iter()
                .map(|link| SankeyLink::new(ids[&link.source], ids[&link.target], link.value))
                .collect::<Vec<_>>();
            let accent = theme.accent;
            SankeyChart::new(nodes, links)
                .id(id)
                .node_label(|node| node.label.clone().into())
                .node_color(move |_| accent)
                .into_any_element()
        }
    };
    Some(
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .children((!title.is_empty()).then(|| {
                div()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(title.clone())
            }))
            .child(
                div()
                    .id(format!("markdown-chart-frame-{block}"))
                    .aria_label(if title.is_empty() {
                        description.clone()
                    } else {
                        format!("{title}: {description}")
                    })
                    .w_full()
                    .h(px(300.0))
                    .child(element),
            )
            .into_any_element(),
    )
}
