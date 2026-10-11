use super::{Chart, Data};
use crate::markdown::style::Theme;
use gpui_kit::component::{
    ActiveTheme,
    chart::{AreaChart, BarChart, CandlestickChart, LineChart, PieChart, RadarChart, SankeyChart},
    plot::shape::SankeyLink,
};
use gpui_kit::{AnyElement, App, IntoElement, SharedString, div, prelude::*, px};

pub(in crate::markdown) fn render(
    source: &str,
    block: usize,
    theme: &Theme,
    cx: &App,
) -> Option<AnyElement> {
    let chart = Chart::parse(source)?;
    let id = format!("markdown-chart-{block}");
    let title = chart.title.clone();
    let description = chart.description();
    let element = match chart.data {
        Data::Line { data } => LineChart::new(data)
            .id(id)
            .x(|point| SharedString::from(point.label.clone()))
            .y(|point| point.value)
            .linear()
            .y_axis(true)
            .grid_columns(4)
            .into_any_element(),
        Data::Bar { data } => BarChart::new(data)
            .id(id)
            .band(|point| SharedString::from(point.label.clone()))
            .value(|point| point.value)
            .value_axis(true)
            .grid_columns(4)
            .into_any_element(),
        Data::Area { data } => AreaChart::new(data)
            .id(id)
            .x(|point| SharedString::from(point.label.clone()))
            .y(|point| point.value)
            .linear()
            .y_axis(true)
            .grid_columns(4)
            .into_any_element(),
        Data::Pie { data } => {
            let colors = [
                cx.theme().chart_1,
                cx.theme().chart_2,
                cx.theme().chart_3,
                cx.theme().chart_4,
                cx.theme().chart_5,
            ];
            PieChart::new(data.into_iter().enumerate())
                .id(id)
                .value(|(_, point)| point.value as f32)
                .label(|(_, point)| point.label.clone().into())
                .color(move |(index, _)| colors[index % colors.len()])
                .label_color(theme.text)
                .into_any_element()
        }
        Data::Radar { data } => RadarChart::new(data)
            .id(id)
            .value(|point| point.value)
            .label(|point| point.label.clone())
            .label_color(theme.text)
            .into_any_element(),
        Data::Candlestick { data } => CandlestickChart::new(data)
            .id(id)
            .x(|point| SharedString::from(point.label.clone()))
            .open(|point| point.open)
            .high(|point| point.high)
            .low(|point| point.low)
            .close(|point| point.close)
            .y_axis(true)
            .grid_columns(4)
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
            SankeyChart::new(nodes, links)
                .id(id)
                .node_label(|node| node.label.clone().into())
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
                    .debug_selector(move || format!("markdown-chart-frame-{block}"))
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
