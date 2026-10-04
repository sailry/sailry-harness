//! Kit 0.7 has no heatmap or stacked BarChart. Its stacked-bar story uses
//! Plot, Stack and Bar; these adapters use the same supported primitives.
//! Packages supply categories,
//! placement, scales and literal tooltip text. Geometry is adapted from Sailry
//! ef5e82ea's statistics charts without retaining usage policy in the host.
mod heatmap;
mod stacked;
#[cfg(test)]
mod tests;

use super::host::sdk::values::decode;
use gpui_kit::{
    component::{
        plot::{
            IntoPlot, Plot, PlotLabel,
            label::{Text, measure_text_width},
            shape::Bar,
            tooltip::{Tooltip, TooltipState},
        },
        *,
    },
    *,
};
use gpui_shell::{HostError, HostModule};
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    label: String,
    value: String,
}

fn tooltip(content: impl IntoElement, cursor: Point<Pixels>, bounds: Bounds<Pixels>) -> AnyElement {
    // Plot's own flip only knows its bounds. Anchoring retains window clipping
    // for tall lists and edge columns using the framework's overlay lifecycle.
    Tooltip::new(cursor, bounds.size)
        .appearance(false)
        .child(
            anchored()
                .position(bounds.origin + cursor)
                .child(div().p_4().child(content)),
        )
        .into_any_element()
}

pub(super) fn module(module: HostModule) -> HostModule {
    let declarations = format!(
        "{}\n{}",
        module.declared().unwrap_or_default(),
        include_str!("charts.d.ts")
    );
    module
        // Kit's script Progress does not expose the native semantic color property.
        .component("ProgressBar", |args, _, cx| {
            let value = args
                .props()
                .get("value")
                .and_then(gpui_shell::HostValue::as_number)
                .unwrap_or(0.);
            let color = match args
                .props()
                .get("color")
                .and_then(gpui_shell::HostValue::as_str)
            {
                Some("chart_1") => cx.theme().chart_1,
                Some("chart_2") => cx.theme().chart_2,
                Some("chart_3") => cx.theme().chart_3,
                Some("chart_4") => cx.theme().chart_4,
                Some("chart_5") => cx.theme().chart_5,
                _ => cx.theme().primary,
            };
            let label = args
                .props()
                .get("label")
                .and_then(gpui_shell::HostValue::as_str)
                .unwrap_or_default()
                .to_owned();
            progress::Progress::new(args.id().to_owned())
                .small()
                .value(value.clamp(0., 100.) as f32)
                .color(color)
                .accessibility_label(label)
                .into_any_element()
        })
        .component("Heatmap", |args, _, _| {
            let Ok(props) = decode(args.props()).and_then(|value| {
                serde_json::from_value::<heatmap::Props>(value)
                    .map_err(|error| HostError::new(error.to_string()))
            }) else {
                return div().into_any_element();
            };
            let Some(chart) = heatmap::Heatmap::new(args.id().to_owned(), props) else {
                return div().into_any_element();
            };
            let id = args.id().to_owned();
            div()
                .debug_selector(move || id.clone())
                .w_full()
                .h(chart.height())
                .child(chart)
                .into_any_element()
        })
        .component("StackedChart", |args, _, _| {
            let Ok(props) = decode(args.props()).and_then(|value| {
                serde_json::from_value::<stacked::Props>(value)
                    .map_err(|error| HostError::new(error.to_string()))
            }) else {
                return div().into_any_element();
            };
            let Some(chart) = stacked::StackedChart::new(args.id().to_owned(), props) else {
                return div().into_any_element();
            };
            let id = args.id().to_owned();
            div()
                .debug_selector(move || id.clone())
                .w_full()
                .h(px(148.))
                .child(chart)
                .into_any_element()
        })
        .declarations(declarations)
}
