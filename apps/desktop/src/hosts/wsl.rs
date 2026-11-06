//! WSL drafts and Kit controls only; discovery and installation use the local Node.
use super::*;
use gpui_kit::component::{
    form::Field,
    select::{Select, SelectEvent, SelectState},
};

pub(super) struct Form {
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    select: Entity<SelectState<Vec<SharedString>>>,
    loading: bool,
    loaded: bool,
    task: Option<Task<()>>,
}

impl Form {
    pub(super) fn new(services: &Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let select = cx.new(|cx| SelectState::new(Vec::<SharedString>::new(), None, window, cx));
        cx.subscribe(&select, |_, _, _: &SelectEvent<Vec<SharedString>>, cx| {
            cx.notify()
        })
        .detach();
        Self {
            client: Arc::new(Client::new(services.local.clone())),
            runtime: services.runtime.clone(),
            select,
            loading: false,
            loaded: false,
            task: None,
        }
    }

    pub(super) fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.loaded && !self.loading {
            self.refresh(window, cx);
        }
    }

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        let client = self.client.clone();
        let job = self.runtime.spawn(async move {
            client
                .execute(client.prepare(Command::ListWslDistributions))
                .await
        });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = job.await.unwrap_or_else(|_| {
                Err(Fault::new(
                    ErrorCode::Unavailable,
                    "WSL distribution discovery is unavailable",
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                this.loading = false;
                match result {
                    Ok(Output::WslDistributions(distributions)) => {
                        let items = distributions
                            .into_iter()
                            .filter(|item| item.version == 2)
                            .map(|item| SharedString::from(item.name))
                            .collect::<Vec<_>>();
                        this.select.update(cx, |select, cx| {
                            select.set_items(items, window, cx);
                        });
                        this.loaded = true;
                    }
                    result => {
                        let detail = match result {
                            Err(error) => crate::feedback::fault_message(&error),
                            _ => "Unexpected WSL discovery result".into(),
                        };
                        crate::feedback::error("host_wsl_load_failed", "", &detail, window, cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn distribution(&self, cx: &App) -> Option<String> {
        (!self.loading)
            .then(|| {
                self.select
                    .read(cx)
                    .selected_value()
                    .map(|value| value.to_string())
            })
            .flatten()
    }

    pub(super) fn body(&self, disabled: bool, cx: &mut Context<Self>) -> AnyElement {
        let empty = if self.loaded {
            "host_wsl_empty"
        } else {
            "host_wsl_distribution"
        };
        v_flex()
            .w_full()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("host_wsl_requirement"))
                    .child(tr("host_wsl_lifetime")),
            )
            .child(
                Field::new().label(tr("host_wsl_distribution")).child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            Select::new(&self.select)
                                .flex_1()
                                .min_w_0()
                                .disabled(disabled || self.loading)
                                .placeholder(tr("host_wsl_distribution"))
                                .accessibility_label(tr("host_wsl_distribution"))
                                .empty(move |_, cx| {
                                    h_flex()
                                        .p_3()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(tr(empty))
                                }),
                        )
                        .child(
                            Button::new("host-wsl-refresh")
                                .label(tr("host_wsl_refresh"))
                                .loading(self.loading)
                                .disabled(disabled || self.loading)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.refresh(window, cx)),
                                ),
                        ),
                ),
            )
            .into_any_element()
    }
}
