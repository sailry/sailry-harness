use crate::{shell::Shell, tr};
use gpui_kit::{component::*, *};
use sailry_client::{Client, ports::Forwarder};
use sailry_protocol::NodeId;
use std::sync::Arc;

#[cfg(test)]
mod fixture;
mod menu;
mod services;
pub(crate) use services::Source;
mod picker;
#[cfg(test)]
mod tests;
mod view;

pub(crate) struct Workspace {
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    port: Entity<input::InputState>,
    local: Entity<input::InputState>,
    entries: Vec<Entry>,
    busy: bool,
    error: Option<&'static str>,
    pending: Option<Task<()>>,
}
struct Entry {
    forwarder: Forwarder,
    _watch: Task<()>,
    url: String,
}

impl Shell {
    pub(crate) fn ports_for(
        &mut self,
        node: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Workspace>> {
        let transport = self.live.as_ref()?.transport_for(node)?;
        let workspace = self.ports.entry(node).or_insert_with(|| {
            let client = Arc::new(Client::new(transport));
            let runtime = cx.global::<crate::backend::Services>().runtime.clone();
            let workspace = cx.new(|cx| Workspace::new(client, runtime, window, cx));
            workspace.update(cx, |_, cx| {
                crate::feedback::observe(window, cx, |view, _| view.error.into_iter().collect());
            });
            cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
            workspace
        });
        Some(workspace.clone())
    }

    pub(crate) fn bind_ports(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(node) = self.live.as_ref().map(|live| live.selected) else {
            return;
        };
        self.ports_for(node, window, cx);
    }
}
impl Workspace {
    pub(crate) fn new(
        client: Arc<Client>,
        runtime: Arc<tokio::runtime::Runtime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            client,
            runtime,
            port: cx.new(|cx| input::InputState::new(window, cx).placeholder("3000")),
            local: cx.new(|cx| {
                input::InputState::new(window, cx)
                    .default_value("0")
                    .placeholder(tr("form_local_port_hint"))
            }),
            entries: Vec::new(),
            busy: false,
            error: None,
            pending: None,
        }
    }

    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let port = self
            .port
            .read(cx)
            .value()
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0);
        let local = self.local.read(cx).value().trim().parse::<u16>().ok();
        let (Some(port), Some(local)) = (port, local) else {
            self.error = Some("port_invalid");
            cx.notify();
            return;
        };
        if self.entries.iter().any(|entry| {
            entry.forwarder.remote_port == port
                && *entry.forwarder.state.borrow() == sailry_client::ports::State::Listening
        }) {
            self.error = Some("port_exists");
            cx.notify();
            return;
        }
        self.start(port, local, None, window, cx);
    }

    fn start(
        &mut self,
        port: u16,
        local: u16,
        service: Option<services::Source>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        self.error = None;
        let client = self.client.clone();
        let source = service.clone();
        let job = self.runtime.spawn(async move {
            match source {
                Some(source) => client.forward_service(source, local).await,
                None => client.forward_port(port, local).await,
            }
        });
        self.pending = Some(cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(Ok(forwarder)) => {
                        let mut status = forwarder.state.clone();
                        let automatic = service.is_some();
                        let local_port = forwarder.local_port;
                        let watch = cx.spawn(async move |this, cx| {
                            while status.changed().await.is_ok() {
                                let closed = *status.borrow_and_update()
                                    == sailry_client::ports::State::Closed;
                                if this
                                    .update(cx, |this, cx| {
                                        if automatic && closed {
                                            this.entries.retain(|entry| {
                                                entry.forwarder.local_port != local_port
                                            });
                                        }
                                        cx.notify();
                                    })
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        });
                        let url = service
                            .as_ref()
                            .map(|source| {
                                services::local_url(&source.service, forwarder.local_port)
                            })
                            .unwrap_or_else(|| {
                                format!("http://127.0.0.1:{}", forwarder.local_port)
                            });
                        this.entries.push(Entry {
                            forwarder,
                            _watch: watch,
                            url: url.clone(),
                        });
                        if service.is_some() {
                            cx.open_url(&url);
                        } else {
                            window.close_dialog(cx);
                            let owner = cx.entity();
                            window.defer(cx, move |window, cx| picker::open(owner, window, cx));
                        }
                    }
                    _ => this.error = Some("port_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn close(&mut self, index: usize, cx: &mut Context<Self>) {
        let entry = self.entries.remove(index);
        self.runtime.spawn(entry.forwarder.close());
        cx.notify();
    }
}
