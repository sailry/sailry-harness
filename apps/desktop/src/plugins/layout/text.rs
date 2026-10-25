//! GPUI aa12fda supplies box sizes, not glyph baselines, to flex layout.
//! Kit 7fc6a4c's scripts expose no mixed-size inline text. Keep native text
//! and align its line boxes using the same shaped metrics as GPUI painting.
use super::super::host::sdk::values::decode;
use gpui_kit::*;
use gpui_shell::{HostError, HostModule};
use serde::Deserialize;

#[derive(Deserialize)]
struct Run {
    text: String,
    size: f32,
    weight: f32,
    color: String,
    #[serde(default)]
    gap: f32,
}

#[derive(Deserialize)]
struct Props {
    runs: Vec<Run>,
    line_height: f32,
    reference_size: Option<f32>,
}

fn measure(
    text: &str,
    size: f32,
    weight: f32,
    line_height: f32,
    window: &Window,
) -> (Pixels, Pixels) {
    let mut style = window.text_style();
    style.font_weight = FontWeight(weight);
    let line = window.text_system().shape_line(
        text.to_owned().into(),
        px(size),
        &[style.to_run(text.len())],
        None,
    );
    let height = window.pixel_snap(px(size * line_height));
    let baseline = (height - line.ascent - line.descent) / 2. + line.ascent;
    (baseline, height - baseline)
}

fn render(props: Props, window: &Window) -> AnyElement {
    let Some(first) = props.runs.first() else {
        return div().into_any_element();
    };
    if props.runs.len() > 64
        || !props.line_height.is_finite()
        || props.line_height <= 0.
        || props
            .reference_size
            .is_some_and(|size| !size.is_finite() || size <= 0.)
        || props.runs.iter().any(|run| {
            run.text.contains(['\r', '\n'])
                || !run.size.is_finite()
                || run.size <= 0.
                || !(100. ..=900.).contains(&run.weight)
                || !run.gap.is_finite()
                || run.gap < 0.
        })
        || props.runs.iter().map(|run| run.text.len()).sum::<usize>() > super::TEXT_LIMIT
    {
        return div().into_any_element();
    }
    let metrics = props
        .runs
        .iter()
        .map(|run| measure(&run.text, run.size, run.weight, props.line_height, window))
        .collect::<Vec<_>>();
    let reference = props
        .reference_size
        .map_or((Pixels::ZERO, Pixels::ZERO), |size| {
            measure("0", size, first.weight, props.line_height, window)
        });
    let (baseline, descent) = metrics
        .iter()
        .fold(reference, |(baseline, descent), &(above, below)| {
            (baseline.max(above), descent.max(below))
        });
    let color = window.text_style().color;
    div()
        .flex()
        .flex_row()
        .items_start()
        .h(baseline + descent)
        .children(
            props
                .runs
                .into_iter()
                .zip(metrics)
                .map(|(run, (above, _))| {
                    div()
                        .ml(px(run.gap))
                        .pt(baseline - above)
                        .text_size(px(run.size))
                        .line_height(relative(props.line_height))
                        .font_weight(FontWeight(run.weight))
                        .text_color(
                            Rgba::try_from(run.color.as_str())
                                .map(Hsla::from)
                                .unwrap_or(color),
                        )
                        .child(run.text)
                }),
        )
        .into_any_element()
}

pub(super) fn extend(module: HostModule) -> HostModule {
    let declarations = format!(
        "{}\n{}",
        module.declared().unwrap_or_default(),
        r#"
            /** One native text line with shaped baselines. reference_size aligns separate lines sharing a parent row. */
            export const TextLine: { new(id: string, props: { runs: { text: string; size: number; weight: number; color: string; gap?: number }[]; line_height: number; reference_size?: number | null }): import("gpui-kit").Element };
        "#
    );
    module
        .component("TextLine", |args, window, _| {
            let Ok(props) = decode(args.props()).and_then(|value| {
                serde_json::from_value::<Props>(value)
                    .map_err(|error| HostError::new(error.to_string()))
            }) else {
                return div().into_any_element();
            };
            render(props, window)
        })
        .declarations(declarations)
}
