//! Session service shortcuts reuse host-owned forwarders, including manual mappings.
use super::*;
pub(crate) use sailry_client::ports::Source;
use sailry_client::ports::State;
use sailry_protocol::process::Service;

pub(super) fn local_url(service: &Service, port: u16) -> String {
    let mut url = url::Url::parse(&service.url).expect("Node supplies a parsed loopback URL");
    url.set_host(Some("127.0.0.1")).unwrap();
    url.set_port(Some(port)).unwrap();
    url.into()
}

impl Workspace {
    pub(crate) fn service_buttons(
        owner: &Entity<Self>,
        sources: Vec<Source>,
        connected: bool,
        cx: &App,
    ) -> Vec<AnyElement> {
        let mut seen = std::collections::BTreeSet::new();
        sources
            .into_iter()
            .filter(|source| seen.insert(source.service.port))
            .map(|source| {
                let port = source.service.port;
                let workspace = owner.read(cx);
                let label = workspace
                    .local_port(port)
                    .map(|local| format!("{port} → {local}"))
                    .unwrap_or_else(|| port.to_string());
                let owner = owner.clone();
                button::Button::new(("session-service", port as usize))
                    .debug_selector(move || format!("session-service-{port}"))
                    .outline()
                    .small()
                    .rounded_full()
                    .px_3()
                    .font_normal()
                    .text_color(cx.theme().muted_foreground)
                    .icon(IconName::Globe)
                    .label(label)
                    .tooltip(tr("port_open_service"))
                    .disabled(!connected || workspace.busy())
                    .on_click(move |_, window, cx| {
                        owner.update(cx, |owner, cx| {
                            owner.open_service(source.clone(), window, cx)
                        })
                    })
                    .into_any_element()
            })
            .collect()
    }

    pub(crate) fn node(&self) -> NodeId {
        self.client.target()
    }

    pub(crate) fn local_port(&self, port: u16) -> Option<u16> {
        self.entries
            .iter()
            .find(|entry| {
                entry.forwarder.remote_port == port
                    && *entry.forwarder.state.borrow() == State::Listening
            })
            .map(|entry| entry.forwarder.local_port)
    }

    pub(crate) fn busy(&self) -> bool {
        self.busy
    }

    pub(crate) fn open_service(
        &mut self,
        source: Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(port) = self.local_port(source.service.port) {
            cx.open_url(&local_url(&source.service, port));
            return;
        }
        if self.busy {
            return;
        }
        self.start(source.service.port, 0, Some(source), window, cx);
    }
}
