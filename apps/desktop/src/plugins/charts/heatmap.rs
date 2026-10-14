use super::*;

/// Largest cell pitch; wider plots keep this pitch and leave the remainder empty.
pub(super) const MAX_PITCH: f32 = 24.;
/// Share of each pitch left as the gap between cells.
pub(super) const GAP: f32 = 0.2;
/// Height of the label band below the cells.
pub(super) const LABELS: f32 = 28.;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Props {
    pub rows: usize,
    pub columns: usize,
    pub cells: Vec<Cell>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Cell {
    pub column: usize,
    pub row: usize,
    pub intensity: f32,
    pub label: String,
    pub label_group: Option<String>,
    pub tooltip: Content,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Content {
    pub heading: String,
    pub title: String,
    pub rows: Vec<Row>,
}
#[derive(IntoPlot)]
pub(super) struct Heatmap {
    id: String,
    pub props: Props,
}
impl Heatmap {
    pub fn new(id: String, props: Props) -> Option<Self> {
        (props.rows > 0
            && props.columns > 0
            && props.cells.iter().all(|cell| {
                cell.row < props.rows
                    && cell.column < props.columns
                    && (0. ..=1.).contains(&cell.intensity)
            }))
        .then_some(Self { id, props })
    }
    /// Pitch that fits every column and row within the plot bounds.
    pub fn pitch(&self, bounds: Bounds<Pixels>) -> f32 {
        let width = bounds.size.width.as_f32() / self.props.columns as f32;
        let height = (bounds.size.height.as_f32() - LABELS) / self.props.rows as f32;
        width.min(height).clamp(0., MAX_PITCH)
    }
    pub fn cell(&self, cell: &Cell, bounds: Bounds<Pixels>) -> Option<Bounds<Pixels>> {
        let pitch = self.pitch(bounds);
        let side = pitch * (1. - GAP);
        // Coordinates are local to the plot, matching Plot's pointer contract.
        (side >= 1.).then(|| {
            Bounds::new(
                point(px(cell.column as f32 * pitch), px(cell.row as f32 * pitch)),
                size(px(side), px(side)),
            )
        })
    }
    /// Scales cells with the available width: Kit has no aspect-ratio plot
    /// container, so a sizing sibling holds the grid ratio and the plot overlays it.
    pub fn frame(self, id: String) -> AnyElement {
        let ratio = self.props.columns as f32 / self.props.rows as f32;
        let height = self.props.rows as f32 * MAX_PITCH;
        div()
            .debug_selector(move || id.clone())
            .relative()
            .w_full()
            .child(div().w_full().aspect_ratio(ratio).max_h(px(height)))
            .child(div().w_full().h(px(LABELS)))
            .child(div().absolute().inset_0().child(self))
            .into_any_element()
    }
}
impl Plot for Heatmap {
    fn id(&self) -> Option<ElementId> {
        Some(SharedString::from(self.id.clone()).into())
    }
    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let pitch = self.pitch(bounds);
        let cells: Vec<_> = self
            .props
            .cells
            .iter()
            .filter_map(|cell| {
                let area = self.cell(cell, bounds)?;
                let color = if cell.intensity == 0. {
                    cx.theme().muted
                } else {
                    cx.theme().chart_2.opacity(cell.intensity)
                };
                Some((area, color))
            })
            .collect();
        Bar::new()
            .data(cells)
            .band_width(pitch * (1. - GAP))
            .cross(|(cell, _)| Some(cell.left().as_f32()))
            .base(|(cell, _)| cell.bottom().as_f32())
            .value(|(cell, _)| Some(cell.top().as_f32()))
            .fill(|(_, color), _, _| *color)
            .corner_radii(px(2.).min(cx.theme().radius))
            .paint(&bounds, window, cx);
        let mut previous: Option<&str> = None;
        let mut right = -1.;
        let labels = self
            .props
            .cells
            .iter()
            .filter_map(|cell| {
                let area = self.cell(cell, bounds)?;
                if cell.label_group.is_some() && previous == cell.label_group.as_deref() {
                    return None;
                }
                previous = cell.label_group.as_deref();
                let text = SharedString::from(cell.label.clone());
                let x = area.left().as_f32();
                let width = measure_text_width(&text, px(10.), window);
                if x < right || x + width > bounds.size.width.as_f32() {
                    return None;
                }
                right = x + width + 8.;
                Some(Text::new(
                    text,
                    point(px(x), px(self.props.rows as f32 * pitch + 8.)),
                    cx.theme().muted_foreground,
                ))
            })
            .collect::<Vec<_>>();
        PlotLabel::new(labels).paint(&bounds, window, cx);
    }
    fn tooltip_state(
        &self,
        position: Point<Pixels>,
        bounds: Bounds<Pixels>,
        _: &App,
    ) -> Option<TooltipState> {
        let index = self.props.cells.iter().position(|cell| {
            self.cell(cell, bounds)
                .is_some_and(|cell| cell.contains(&position))
        })?;
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
        let values = &self.props.cells.get(state.index)?.tooltip;
        let id = format!("{}-tooltip", self.id);
        let mut content = v_flex()
            .popover_style(cx)
            .p_3()
            .text_sm()
            .debug_selector(move || id.clone())
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(values.heading.clone()),
                    )
                    .child(div().font_semibold().child(values.title.clone())),
            );
        if !values.rows.is_empty() {
            content = content.child(
                v_flex()
                    .mt_2()
                    .pt_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .children(values.rows.iter().map(|row| {
                        h_flex()
                            .gap_3()
                            .py_1()
                            .min_w(px(220.))
                            .max_w(px(280.))
                            .child(div().flex_1().min_w_0().truncate().child(row.label.clone()))
                            .child(div().font_semibold().child(row.value.clone()))
                    })),
            );
        }
        Some(tooltip(content, cursor, bounds))
    }
}
