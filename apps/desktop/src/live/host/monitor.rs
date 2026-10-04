use super::processes::Processes;
use super::*;
use gpui_kit::component::{
    alert::Alert,
    group_box::{GroupBox, GroupBoxVariants},
    input::{Input, InputEvent, InputState},
    table::{DataTable, TableState},
};
use sailry_client::host::View as HostView;

pub(crate) struct Monitor {
    pub(super) node: NodeId,
    pub(super) view: HostView,
    pub(super) disk: Option<String>,
    pub(super) gpu: Option<String>,
    pub(super) table: Entity<TableState<Processes>>,
    pub(super) focus: FocusHandle,
    pub(super) client: Arc<Client>,
    pub(super) runtime: Arc<tokio::runtime::Runtime>,
    pub(super) stopping: bool,
    search: Entity<InputState>,
    stop: CancellationToken,
    _watch: Task<()>,
    _search: Subscription,
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Shell {
    pub(crate) fn bind_host_monitor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(live) = self
            .live
            .as_ref()
            .filter(|_| self.layout.sidebar_open || self.page == crate::preview::Page::Host)
        else {
            self.host_monitor = None;
            return;
        };
        if self
            .host_monitor
            .as_ref()
            .is_some_and(|monitor| monitor.read(cx).node == live.selected)
        {
            return;
        }
        let monitor = cx.new(|cx| {
            Monitor::new(
                live.transport.clone(),
                live.services.runtime.clone(),
                window,
                cx,
            )
        });
        cx.observe(&monitor, |_, _, cx| cx.notify()).detach();
        self.host_monitor = Some(monitor);
    }
}

impl Monitor {
    pub(crate) fn percentages(&self) -> [Option<f32>; 2] {
        let sample = self
            .view
            .sample
            .as_ref()
            .filter(|_| self.view.error.is_none());
        [
            sample
                .and_then(|sample| sample.cpu_basis_points)
                .map(|value| value as f32 / 100.),
            sample
                .and_then(|sample| sample.memory.as_ref())
                .filter(|memory| memory.total_bytes > 0)
                .map(|memory| {
                    (memory.total_bytes - memory.available_bytes) as f32 * 100.
                        / memory.total_bytes as f32
                }),
        ]
    }

    pub(super) fn new(
        transport: Arc<dyn Transport>,
        runtime: Arc<tokio::runtime::Runtime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let node = transport.target();
        let client = Arc::new(Client::new(transport));
        let watching = client.clone();
        let stop = CancellationToken::new();
        let cancellation = stop.clone();
        let (sender, mut receiver) = tokio::sync::watch::channel(HostView::default());
        runtime.spawn(async move {
            watching.watch_host(sender, cancellation).await;
        });
        let owner = cx.entity().downgrade();
        let table = cx.new(|cx| {
            TableState::new(Processes::new(owner), window, cx)
                .col_movable(false)
                .col_selectable(false)
                .row_selectable(false)
        });
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr("host_process_search")));
        let search_events = cx.subscribe(&search, |this, input, event, cx| {
            if matches!(event, InputEvent::Change) {
                let query = input.read(cx).value().to_lowercase();
                this.table.update(cx, |table, cx| {
                    table.delegate_mut().filter(query);
                    cx.notify();
                });
            }
        });
        let watch = cx.spawn(async move |this, cx| {
            while receiver.changed().await.is_ok() {
                let view = receiver.borrow_and_update().clone();
                if this.update(cx, |this, cx| this.accept(view, cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            node,
            focus: cx.focus_handle(),
            client,
            runtime,
            stopping: false,
            view: HostView::default(),
            disk: None,
            gpu: None,
            table,
            search,
            stop,
            _watch: watch,
            _search: search_events,
        }
    }

    pub(super) fn accept(&mut self, view: HostView, cx: &mut Context<Self>) {
        if let Some(sample) = &view.sample {
            if self
                .disk
                .as_ref()
                .is_none_or(|mount| !sample.disks.iter().any(|disk| &disk.mount == mount))
            {
                self.disk = sample
                    .disks
                    .iter()
                    .find(|disk| disk.mount == "/")
                    .or(sample.disks.first())
                    .map(|disk| disk.mount.clone());
            }
            if self
                .gpu
                .as_ref()
                .is_none_or(|id| !sample.gpus.iter().any(|gpu| &gpu.id == id))
            {
                self.gpu = sample.gpus.first().map(|gpu| gpu.id.clone());
            }
        }
        self.table.update(cx, |table, cx| {
            table.delegate_mut().update(view.clone());
            cx.notify();
        });
        self.view = view;
        cx.notify();
    }
}

impl Render for Monitor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("host-monitor")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::process_action))
            .gap_6()
            .w_full()
            .min_w_0()
            .debug_selector(|| "host-monitor".into())
            .when(self.view.error.is_some(), |body| {
                body.child(div().debug_selector(|| "host-sampling-error".into()).child(
                    Alert::warning("host-sampling-error", tr("host_sampling_failed")).small(),
                ))
            })
            .child(self.charts(window, cx))
            .child(
                GroupBox::new()
                    .id("host-process-card")
                    .fill()
                    .min_w_0()
                    .content_style(
                        StyleRefinement::default()
                            .min_w_0()
                            .gap_3()
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded(cx.theme().radius_lg),
                    )
                    .child(
                        div()
                            .w_48()
                            .debug_selector(|| "host-process-search".into())
                            .child(Input::new(&self.search).aria_label(tr("host_process_search"))),
                    )
                    .when(
                        self.view
                            .sample
                            .as_ref()
                            .is_some_and(|sample| sample.process_count > sample.processes.len()),
                        |body| {
                            body.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(tr("host_process_limited")),
                            )
                        },
                    )
                    .child(
                        div()
                            .h(px(360.))
                            .w_full()
                            .min_w_0()
                            .debug_selector(|| "host-processes".into())
                            .on_prepaint({
                                let table = self.table.clone();
                                move |bounds, _, cx| {
                                    table.update(cx, |table, cx| {
                                        let width = bounds.size.width.max(px(600.));
                                        if (table.delegate().width - width).abs() > px(1.) {
                                            table.delegate_mut().width = width;
                                            table.refresh(cx);
                                        }
                                    });
                                }
                            })
                            .child(DataTable::new(&self.table).bordered(false)),
                    ),
            )
    }
}
