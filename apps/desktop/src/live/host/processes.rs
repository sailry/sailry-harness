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
        5
    }
    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }
    fn column(&self, index: usize, _: &App) -> Column {
        let (key, label, width) = match index {
            0 => ("name", "host_process_name", 200.),
            1 => ("cpu", "metrics_cpu", 85.),
            2 => ("memory", "metrics_memory", 105.),
            3 => ("history", "host_cpu_history", 135.),
            _ => ("pid", "host_pid", 75.),
        };
        Column::new(key, tr(label))
            .width(px(width) * (self.width.as_f32() / 600.).max(1.))
            .movable(false)
            .map(|column| {
                if index == 3 {
                    column
                } else if index == self.column {
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
        if column == 3 {
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
                .map(|(index, value)| (index.to_string(), value.unwrap_or_default() as f64 / 100.))
                .collect();
            return div()
                .h_6()
                .w_full()
                .when(points.len() >= 2, |body| {
                    body.child(
                        AreaChart::new(points)
                            .x(|point| point.0.clone())
                            .y(|point| point.1)
                            .stroke(cx.theme().chart_2)
                            .fill(cx.theme().chart_2.opacity(0.12))
                            .linear()
                            .x_axis(false)
                            .grid(false),
                    )
                })
                .into_any_element();
        }
        let text = match column {
            0 => process.name.clone(),
            1 => super::resources::percent(process.cpu_basis_points),
            2 => super::resources::bytes(process.memory_bytes),
            _ => process.pid.to_string(),
        };
        div()
            .w_full()
            .when(row == 0, |cell| {
                cell.debug_selector(move || format!("host-process-cell-{column}"))
            })
            .truncate()
            .text_sm()
            .child(text)
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
