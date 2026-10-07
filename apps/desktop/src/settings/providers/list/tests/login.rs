use super::super::super::authorization_support as support;
use super::*;
use gpui_kit::test::TestWindowExt as _;
use sailry_link::{Admission, Link, NetworkScope, Pending, Subscription, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::{
    Fault, NodeId, Request, RequestId, Topic, Update,
    conversation::{Provider, login::State},
};
use std::sync::{Arc, Mutex, atomic::AtomicU8};

mod cancellation;
mod recovery;

fn close_dialog(visual: &mut VisualTestContext) {
    draw(visual);
    visual.update(|window, cx| {
        let close = window.within("dialog-close").find("close");
        assert_eq!(close.label(), Some("Close"));
        assert_eq!(close.role(), Some(Role::Button));
        assert!(close.visible());
        window.within("dialog-close").click("close", cx);
    });
    draw(visual);
}

struct Recording {
    inner: Arc<dyn Transport>,
    requests: Mutex<Vec<Request>>,
}

impl Transport for Recording {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        self.requests.lock().unwrap().push(request.clone());
        self.inner.dispatch(request)
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
}

struct Fixture {
    runtime: Arc<tokio::runtime::Runtime>,
    node: Node,
    controller: Link,
    transport: Arc<dyn Transport>,
    recording: Arc<Recording>,
    mode: Arc<AtomicU8>,
    server: support::Server,
    _directory: tempfile::TempDir,
}

impl Fixture {
    fn new(remote: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let mode = Arc::new(AtomicU8::new(0));
        let server = runtime.block_on(support::server(mode.clone()));
        let node = runtime
            .block_on(Node::start_with_authorization(
                directory.path().join("node"),
                &server.endpoint,
            ))
            .unwrap();
        let controller = runtime
            .block_on(Link::controller(
                directory.path().join("controller"),
                NetworkScope::default(),
            ))
            .unwrap();
        let address = runtime
            .block_on(
                controller
                    .handle()
                    .pair(node.link().invite().unwrap().ticket()),
            )
            .unwrap();
        let inner = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let recording = Arc::new(Recording {
            inner,
            requests: Mutex::new(Vec::new()),
        });
        Self {
            runtime,
            node,
            controller,
            transport: recording.clone(),
            recording,
            mode,
            server,
            _directory: directory,
        }
    }

    fn execute(&self, command: Command) -> Output {
        let client = Client::new(self.transport.clone());
        self.runtime
            .block_on(client.execute(client.prepare(command)))
            .unwrap()
    }

    fn state(&self, attempt: RequestId) -> State {
        self.runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let client = Client::new(self.transport.clone());
                let mut updates = client.subscribe_login(attempt).await.unwrap();
                let Update::ProviderLogin(update) = updates.next().await.unwrap() else {
                    panic!("login update expected")
                };
                update.state
            })
            .await
            .expect("login snapshot deadline")
        })
    }

    fn provider(&self) -> Provider {
        let Output::Providers(providers) = self.execute(Command::ListProviders) else {
            panic!("providers expected")
        };
        providers.into_iter().next().unwrap()
    }

    fn mount<'a>(
        &self,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Workspace>, &'a mut VisualTestContext) {
        let saved = self.provider();
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let workspace = cx.new(|cx| Workspace::new(window, cx));
            workspace.update(cx, |workspace, cx| {
                workspace.bind_providers(
                    self.transport.clone(),
                    self.runtime.clone(),
                    "Authorization Node".into(),
                    cx,
                );
                workspace.select(crate::settings::Section::Providers, cx);
            });
            owner = Some(workspace.clone());
            Root::new(workspace, window, cx)
        });
        let owner = owner.unwrap();
        wait(visual, |cx| {
            owner.read(cx).providers.channels.len() == 1
                && owner.read(cx).providers.channels[0].preset == Preset::ChatGpt
                && owner.read(cx).provider_link.as_ref().is_some_and(|live| {
                    live.connected
                        && !live.pending
                        && live
                            .providers
                            .get(&0)
                            .is_some_and(|provider| provider.id == saved.id)
                })
        });
        (owner, visual)
    }

    fn begin(&self, visual: &mut VisualTestContext) -> (RequestId, String) {
        let before = self.recording.requests.lock().unwrap().len();
        tap(visual, "provider-login-0");
        wait(visual, |_| {
            self.recording.requests.lock().unwrap()[before..]
                .iter()
                .any(|request| matches!(request.command, Command::BeginProviderLogin { .. }))
        });
        let attempt = self.recording.requests.lock().unwrap()[before..]
            .iter()
            .find(|request| matches!(request.command, Command::BeginProviderLogin { .. }))
            .unwrap()
            .id;
        let deadline = Instant::now() + Duration::from_secs(10);
        while visual.debug_bounds("provider-login-waiting").is_none() {
            draw(visual);
            assert!(Instant::now() < deadline, "browser readiness deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
        draw(visual);
        let State::Browser {
            authorization_url, ..
        } = self.state(attempt)
        else {
            panic!("browser authorization expected")
        };
        assert_eq!(
            visual.opened_url().as_deref(),
            Some(authorization_url.as_str())
        );
        (attempt, authorization_url)
    }

    fn callback(&self, authorization: &str) -> tokio::task::JoinHandle<String> {
        let callback = support::callback(authorization);
        self.runtime.spawn(async move {
            reqwest::get(callback.expose())
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .text()
                .await
                .unwrap()
        })
    }

    fn page_result(
        &self,
        response: tokio::task::JoinHandle<String>,
        visual: &mut VisualTestContext,
        outcome: &str,
    ) {
        wait(visual, |_| response.is_finished());
        let html = self.runtime.block_on(response).unwrap();
        assert!(html.contains(&format!("data-outcome=\"{outcome}\"")));
    }

    fn close(self) {
        self.runtime.block_on(self.controller.close()).unwrap();
        self.runtime.block_on(self.node.shutdown()).unwrap();
    }
}

#[gpui::test]
fn authorizes_saved_provider_without_saving(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let provider = support::provider(Authentication::ChatGpt, ModelApi::Responses);
        fixture.execute(Command::SaveProvider {
            provider,
            expected_revision: 0,
            secret: None,
        });
        let saved = fixture.provider();
        let (owner, visual) = fixture.mount(cx);
        assert!(visual.debug_bounds("provider-login-0").is_some());
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        fixture.recording.requests.lock().unwrap().clear();
        tap(visual, "provider-login-0");
        let deadline = Instant::now() + Duration::from_secs(10);
        while visual.debug_bounds("provider-login-waiting").is_none() {
            draw(visual);
            assert!(Instant::now() < deadline, "browser readiness deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
        draw(visual);
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        assert!(visual.debug_bounds("provider-login-loading").is_none());
        assert!(visual.debug_bounds("provider-login-footer").is_none());
        assert!(visual.debug_bounds("provider-login-cancel").is_none());
        assert!(visual.debug_bounds("provider-login").unwrap().size.width <= px(520.));
        let requests = fixture.recording.requests.lock().unwrap().clone();
        let beginnings: Vec<_> = requests
            .iter()
            .filter(|request| matches!(request.command, Command::BeginProviderLogin { .. }))
            .collect();
        assert_eq!(beginnings.len(), 1);
        let request = beginnings[0];
        let Command::BeginProviderLogin {
            provider,
            expected_revision,
            redirect,
        } = &request.command
        else {
            unreachable!()
        };
        assert_eq!(*provider, saved.id);
        assert_eq!(*expected_revision, saved.revision);
        let redirect = reqwest::Url::parse(redirect.as_ref().unwrap()).unwrap();
        assert_eq!(redirect.scheme(), "http");
        assert_eq!(redirect.host_str(), Some("127.0.0.1"));
        assert!(redirect.port().is_some());
        assert_eq!(redirect.path(), "/auth/callback");
        let State::Browser {
            authorization_url, ..
        } = fixture.state(request.id)
        else {
            panic!("browser authorization expected")
        };
        assert!(authorization_url.starts_with(&fixture.server.endpoint));
        assert_eq!(
            visual.opened_url().as_deref(),
            Some(authorization_url.as_str())
        );
        tap(visual, "provider-login-open");
        assert_eq!(
            visual.opened_url().as_deref(),
            Some(authorization_url.as_str())
        );
        close_dialog(visual);
        wait(visual, |_| fixture.state(request.id) == State::Cancelled);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(fixture.provider(), saved);
        let requests = fixture.recording.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| matches!(request.command, Command::BeginProviderLogin { .. }))
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| matches!(request.command, Command::CancelProviderLogin { .. }))
                .count(),
            1
        );
        assert!(!requests.iter().any(|request| matches!(
            request.command,
            Command::SaveProvider { .. } | Command::ReadProviderKey { .. }
        )));
        drop(requests);
        visual.update(|window, _| window.remove_window());
        visual.run_until_parked();
        drop(owner);
        fixture.close();
    }
}
