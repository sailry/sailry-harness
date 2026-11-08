//! Kit 4424427's PieChart has no partial sweep or independent segment progress.
//! This score gauge composes Kit's Plot and Arc primitives with themed labels.
use super::*;
use gpui_kit::component::{
    plot::{
        IntoPlot, Plot,
        shape::{Arc, ArcData},
    },
    *,
};

const START: f32 = -135.;
const SWEEP: f32 = 270.;
const GAP: f32 = 3.;
const AXES: [&str; 5] = [
    "model_score_axis_backend",
    "model_score_axis_frontend",
    "model_score_axis_knowledge",
    "model_score_axis_aesthetic",
    "model_score_axis_maintainability",
];

#[derive(IntoPlot)]
struct Ring {
    axes: [u8; 5],
}

impl Plot for Ring {
    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let radius = bounds.size.height.as_f32() * 0.34;
        let shape = Arc::new().inner_radius(radius * 0.84).outer_radius(radius);
        for (index, value) in self.axes.iter().enumerate() {
            let start = START + index as f32 * SWEEP / AXES.len() as f32 + GAP / 2.;
            let end = start + SWEEP / AXES.len() as f32 - GAP;
            let background = ArcData::new(value, index, 100., start.to_radians(), end.to_radians());
            shape.paint(
                &background,
                cx.theme().primary.opacity(0.12),
                &bounds,
                window,
            );
            let progress = ArcData::new(
                value,
                index,
                f32::from(*value),
                start.to_radians(),
                (start + (end - start) * f32::from(*value) / 100.).to_radians(),
            );
            shape.paint(&progress, cx.theme().primary, &bounds, window);
        }
    }
}

/// The gauge opens downward, so the card keeps only the rows the arc and its
/// labels occupy instead of the empty lower quarter of the square plot.
pub(super) fn render(score: &catalog::EffortScore, cx: &App) -> AnyElement {
    div()
        .w_full()
        .h(rems(12.))
        .overflow_hidden()
        .child(gauge(score, cx))
        .into_any_element()
}

fn gauge(score: &catalog::EffortScore, cx: &App) -> Div {
    div()
        .relative()
        .w_full()
        .h(rems(15.5))
        .child(div().absolute().inset_0().child(Ring { axes: score.axes }))
        .child(
            v_flex()
                .absolute()
                .left(relative(0.27))
                .top(relative(0.38))
                .w(relative(0.46))
                .items_center()
                .gap_1()
                .child(
                    div()
                        .text_3xl()
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(format!("{:.1}", score.overall)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("model_score_overall")),
                ),
        )
        .children(
            AXES.iter()
                .zip(score.axes)
                .enumerate()
                .map(|(index, (key, value))| {
                    let angle =
                        (START + (index as f32 + 0.5) * SWEEP / AXES.len() as f32).to_radians();
                    v_flex()
                        .absolute()
                        .top(relative(0.5 - angle.cos() * 0.44 - 0.075))
                        .w(relative(0.25))
                        .when(index < 2, |label| label.left_0().items_end().text_right())
                        .when(index == 2, |label| {
                            label
                                .left(relative(0.35))
                                .w(relative(0.3))
                                .items_center()
                                .text_center()
                        })
                        .when(index > 2, |label| label.right_0().items_start().text_left())
                        .gap_0p5()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr(key)),
                        )
                        .child(
                            div()
                                .text_sm()
                                .font_medium()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(value.to_string()),
                        )
                }),
        )
}
