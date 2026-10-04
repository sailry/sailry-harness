use super::*;
use gpui_kit::component::plot::{
    AxisLabelSide, AxisText, Grid, PlotAxis,
    scale::{Scale, ScaleBand, ScaleLinear},
    shape::{Stack, StackSeries},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Props {
    pub series: Vec<Series>,
    pub buckets: Vec<Bucket>,
    pub maximum: f64,
    pub ticks: Vec<Tick>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Series {
    pub id: String,
    pub color: Color,
}
#[derive(Clone, Copy, Deserialize)]
pub(super) enum Color {
    #[serde(rename = "chart_1")]
    Chart1,
    #[serde(rename = "chart_2")]
    Chart2,
    #[serde(rename = "chart_3")]
    Chart3,
    #[serde(rename = "chart_4")]
    Chart4,
    #[serde(rename = "chart_5")]
    Chart5,
}
impl Color {
    fn value(self, cx: &App) -> Hsla {
        match self {
            Self::Chart1 => cx.theme().chart_1,
            Self::Chart2 => cx.theme().chart_2,
            Self::Chart3 => cx.theme().chart_3,
            Self::Chart4 => cx.theme().chart_4,
            Self::Chart5 => cx.theme().chart_5,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Bucket {
    pub label: String,
    pub values: Option<Vec<f32>>,
    pub tooltip: Content,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Content {
    pub heading: String,
    pub rows: Vec<Row>,
    pub footer: Row,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Tick {
    pub value: f64,
    pub label: String,
}
#[derive(IntoPlot)]
pub(super) struct StackedChart {
    id: String,
    pub props: Props,
    pub series: Vec<StackSeries<usize>>,
}
impl StackedChart {
    pub fn new(id: String, props: Props) -> Option<Self> {
        if !props.maximum.is_finite()
            || props.maximum < 0.
            || props.ticks.iter().any(|tick| !tick.value.is_finite())
            || props.buckets.iter().any(|bucket| {
                bucket.tooltip.rows.len() != props.series.len()
                    || bucket.values.as_ref().is_some_and(|values| {
                        values.len() != props.series.len()
                            || values.iter().any(|value| !value.is_finite() || *value < 0.)
                    })
            })
        {
            return None;
        }
        let keys = props
            .series
            .iter()
            .map(|series| series.id.clone())
            .collect::<Vec<_>>();
        let values = props
            .buckets
            .iter()
            .map(|bucket| bucket.values.clone())
            .collect::<Vec<_>>();
        let series = Stack::new()
            .data(0..props.buckets.len())
            .keys(keys.clone())
            .value(move |index, key| {
                let column = keys.iter().position(|candidate| candidate == key)?;
                values[*index]
                    .as_ref()
                    .and_then(|values| values.get(column))
                    .copied()
            })
            .series();
        Some(Self { id, props, series })
    }
    pub fn area(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
        Bounds::new(
            point(px(48.), px(8.)),
            size(
                (bounds.size.width - px(48.)).max(px(0.)),
                (bounds.size.height - px(36.)).max(px(0.)),
            ),
        )
    }
    pub fn scale(&self, area: Bounds<Pixels>) -> ScaleBand<usize> {
        ScaleBand::new(0..self.props.buckets.len(), [0., area.size.width.as_f32()])
            .padding_inner(0.2)
            .padding_outer(0.2)
    }
}
impl Plot for StackedChart {
    fn id(&self) -> Option<ElementId> {
        Some(SharedString::from(self.id.clone()).into())
    }
    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let area = Self::area(bounds);
        if area.size.width <= px(0.) || area.size.height <= px(0.) {
            return;
        }
        let plot = Bounds::new(bounds.origin + area.origin, area.size);
        let x = self.scale(area);
        let y = ScaleLinear::new(
            vec![0., self.props.maximum.max(1.)],
            [area.size.height.as_f32(), 0.],
        );
        Grid::new()
            .y(self
                .props
                .ticks
                .iter()
                .filter_map(|tick| y.tick(&tick.value))
                .map(px)
                .collect::<Vec<_>>())
            .stroke(cx.theme().border)
            .paint(&plot, window);
        for (series, style) in self.series.iter().zip(&self.props.series) {
            let x = x.clone();
            let base = y.clone();
            let value = y.clone();
            let color = style.color.value(cx);
            Bar::new()
                .data(series.points.clone())
                .band_width(x.band_width())
                .cross(move |point| x.tick(&point.data))
                .base(move |point| base.tick(&f64::from(point.y0)).unwrap_or(0.))
                .value(move |point| value.tick(&f64::from(point.y1)))
                .fill(move |_, _, _| color)
                .paint(&plot, window, cx);
        }
        let font_size = cx.theme().font_size * 0.75;
        let mut right = area.left();
        let labels = self
            .props
            .buckets
            .iter()
            .enumerate()
            .filter_map(|(index, bucket)| {
                let text = SharedString::from(bucket.label.clone());
                let width = px(measure_text_width(&text, font_size, window));
                let center = area.left() + px(x.tick(&index)? + x.band_width() / 2.);
                let left = (center - width / 2.).max(area.left());
                if left < right || left + width > area.right() {
                    return None;
                }
                right = left + width + px(12.);
                Some(AxisText::new(text, left, cx.theme().muted_foreground).font_size(font_size))
            })
            .collect::<Vec<_>>();
        PlotAxis::new()
            .x(area.bottom())
            .x_label(labels)
            .y(area.left())
            .y_label_side(AxisLabelSide::Start)
            .y_label(self.props.ticks.iter().map(|tick| {
                AxisText::new(
                    SharedString::from(tick.label.clone()),
                    area.top() + px(y.tick(&tick.value).unwrap_or(0.)),
                    cx.theme().muted_foreground,
                )
                .font_size(font_size)
                .align(TextAlign::Right)
            }))
            .stroke(cx.theme().border)
            .paint(&bounds, window, cx);
    }
    fn tooltip_state(
        &self,
        position: Point<Pixels>,
        bounds: Bounds<Pixels>,
        _: &App,
    ) -> Option<TooltipState> {
        let area = Self::area(bounds);
        if self.props.buckets.is_empty() || !area.contains(&position) {
            return None;
        }
        let scale = self.scale(area);
        let index =
            scale.nearest_index((position.x - area.left()).as_f32() - scale.band_width() / 2.);
        Some(TooltipState::new(index, position, vec![]))
    }
    fn tooltip(
        &self,
        state: &TooltipState,
        cursor: Point<Pixels>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let values = &self.props.buckets.get(state.index)?.tooltip;
        let id = format!("{}-tooltip", self.id);
        let content = v_flex()
            .popover_style(cx)
            .p_3()
            .gap_2()
            .text_sm()
            .debug_selector(move || id.clone())
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(values.heading.clone()),
            )
            .children(
                values
                    .rows
                    .iter()
                    .zip(&self.props.series)
                    .map(|(row, series)| {
                        h_flex()
                            .gap_3()
                            .child(div().size_2().rounded_full().bg(series.color.value(cx)))
                            .child(div().flex_1().child(row.label.clone()))
                            .child(div().font_semibold().child(row.value.clone()))
                    }),
            )
            .child(
                h_flex()
                    .gap_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .pt_2()
                    .child(div().flex_1().child(values.footer.label.clone()))
                    .child(div().font_semibold().child(values.footer.value.clone())),
            );
        Some(tooltip(content, cursor, bounds))
    }
}
