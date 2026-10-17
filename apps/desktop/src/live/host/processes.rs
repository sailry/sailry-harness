use super::resources::{Tones, bytes, mono, percent};
use super::*;
use gpui_kit::component::{
    chart::AreaChart,
    table::{Column, ColumnSort, TableDelegate, TableState},
};
use sailry_client::host::View as HostView;
use sailry_protocol::host::metrics::Process;

pub(super) struct Processes {
    owner: Option<WeakEntity<Monitor>>,
    view: HostView,
    rows: Vec<usize>,
    query: String,
    column: usize,
    order: ColumnSort,
    /// The largest sampled resident memory, scaling every memory bar.
    peak: u64,
    pub(super) width: Pixels,
}

impl Default for Processes {
    fn default() -> Self {
        Self {
            owner: None,
            view: HostView::default(),
            rows: Vec::new(),
            query: String::new(),
            column: 1,
            order: ColumnSort::Descending,
            peak: 0,
            width: px(600.),
        }
    }
}

impl Processes {
    pub(super) fn new(owner: WeakEntity<Monitor>) -> Self {
        Self {
            owner: Some(owner),
            ..Self::default()
        }
    }
    pub(super) fn update(&mut self, view: HostView) {
        self.peak = view
            .sample
            .iter()
            .flat_map(|sample| &sample.processes)
            .map(|process| process.memory_bytes)
            .max()
            .unwrap_or_default();
        self.view = view;
        self.sort();
    }
    pub(super) fn filter(&mut self, query: String) {
        self.query = query;
        self.sort();
    }
    fn process(&self, row: usize) -> Option<&Process> {
        self.view
            .sample
            .as_ref()?
            .processes
            .get(*self.rows.get(row)?)
    }
    fn sort(&mut self) {
        let Some(sample) = &self.view.sample else {
            self.rows.clear();
            return;
        };
        self.rows = sample
            .processes
            .iter()
            .enumerate()
            .filter(|(_, process)| {
                process.name.to_lowercase().contains(&self.query)
                    || process.pid.to_string().contains(&self.query)
            })
            .map(|(index, _)| index)
            .collect();
        self.rows.sort_by(|left, right| {
            let left = &sample.processes[*left];
            let right = &sample.processes[*right];
            let order = match self.column {
                0 => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
                1 => left.cpu_basis_points.cmp(&right.cpu_basis_points),
                2 => left.memory_bytes.cmp(&right.memory_bytes),
                _ => left.pid.cmp(&right.pid),
            };
            (if self.order == ColumnSort::Descending {
                order.reverse()
            } else {
                order
            })
            .then_with(|| left.pid.cmp(&right.pid))
        });
    }
}

impl TableDelegate for Processes {
    fn render_tr(
        &mut self,
        row: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let process = self.process(row).cloned();
        let owner = self.owner.clone();
        div()
            .id(("host-process", row))
            .debug_selector(move || format!("host-process-row-{row}"))
            .capture_any_mouse_down(move |event, window, cx| {
                if event.button != MouseButton::Right {
                    return;
                }
                // Intercept before the table records a toolkit context-menu selection.
                if let (Some(owner), Some(process)) = (&owner, &process) {
                    let _ = owner.update(cx, |this, cx| {
                        this.process_menu(process, event, window, cx);
                    });
                }
            })
    }
    fn columns_count(&self, _: &App) -> usize {
        4
    }
    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }
    fn column(&self, index: usize, _: &App) -> Column {
        let (key, label, width) = match index {
            0 => ("name", "host_process_name", 210.),
            1 => ("cpu", "metrics_cpu", 200.),
            2 => ("memory", "metrics_memory", 130.),
            _ => ("pid", "host_pid", 60.),
        };
        Column::new(key, tr(label))
            .width(px(width) * (self.width.as_f32() / 600.).max(1.))
            .movable(false)
            .when(index > 0, |column| column.text_right())
            .map(|column| {
                if index == self.column {
                    column.sort(self.order)
                } else {
                    column.sortable()
                }
            })
    }
    fn perform_sort(
        &mut self,
        index: usize,
        order: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        self.column = index;
        self.order = order;
        self.sort();
        cx.notify();
    }
    fn render_td(
        &mut self,
        row: usize,
        column: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(process) = self.process(row) else {
            return div().into_any_element();
        };
        let cell = match column {
            0 => h_flex()
                .gap_2p5()
                .child(
                    h_flex()
                        .size(px(22.))
                        .flex_shrink_0()
                        .justify_center()
                        .rounded_full()
                        .bg(cx.theme().muted)
                        .text_xs()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            process
                                .name
                                .chars()
                                .next()
                                .map(|initial| initial.to_uppercase().to_string())
                                .unwrap_or_default(),
                        ),
                )
                .child(div().min_w_0().truncate().child(process.name.clone())),
            1 => {
                let points: Vec<_> = self
                    .view
                    .processes
                    .get(&(process.pid, process.started_at_secs))
                    .into_iter()
                    .flatten()
                    .rev()
                    .take_while(|value| value.is_some())
                    .copied()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .enumerate()
                    .map(|(index, value)| {
                        (index.to_string(), value.unwrap_or_default() as f64 / 100.)
                    })
                    .collect();
                let tone = Tones::of(cx).cpu;
                h_flex()
                    .justify_end()
                    .gap_3()
                    .child(div().w(px(120.)).h_6().flex_shrink_0().when(
                        points.len() >= 2,
                        |body| {
                            body.child(
                                AreaChart::new(points)
                                    .id(("host-process-cpu", row))
                                    .appear(false)
                                    .interactive(false)
                                    .x(|point| point.0.clone())
                                    .y(|point| point.1)
                                    .stroke(tone)
                                    .fill(tone.opacity(0.14))
                                    .linear()
                                    .x_axis(false)
                                    .grid(false),
                            )
                        },
                    ))
                    .child(mono(
                        div()
                            .w(px(50.))
                            .flex_shrink_0()
                            .text_right()
                            .child(percent(process.cpu_basis_points)),
                        cx,
                    ))
            }
            2 => {
                let share = if self.peak == 0 {
                    0.
                } else {
                    process.memory_bytes as f32 / self.peak as f32
                };
                let tones = Tones::of(cx);
                h_flex()
                    .justify_end()
                    .gap_2p5()
                    .child(
                        div()
                            .w(px(44.))
                            .h_1()
                            .flex_shrink_0()
                            .rounded(px(2.))
                            .overflow_hidden()
                            .bg(tones.track)
                            .child(div().h_full().w(relative(share)).bg(tones.gpu)),
                    )
                    .child(mono(
                        div()
                            .w(px(76.))
                            .flex_shrink_0()
                            .text_right()
                            .child(bytes(process.memory_bytes)),
                        cx,
                    ))
            }
            _ => h_flex().justify_end().child(mono(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(process.pid.to_string()),
                cx,
            )),
        };
        cell.w_full()
            .min_w_0()
            .text_sm()
            .when(row == 0, |cell| {
                cell.debug_selector(move || format!("host-process-cell-{column}"))
            })
            .into_any_element()
    }
    fn loading(&self, _: &App) -> bool {
        self.view.sample.is_none() && self.view.error.is_none()
    }
    fn render_last_empty_col(
        &mut self,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
    }
    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        crate::empty_state::list(
            IconName::Cpu,
            if self.query.is_empty() {
                "host_process_empty"
            } else {
                "host_process_no_matches"
            },
            cx,
        )
    }
}
