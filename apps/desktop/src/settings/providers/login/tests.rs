use super::*;
use core::prelude::v1::test;
use sailry_client::Client;
use sailry_link::{Link, NetworkScope, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::{Authentication, conversation::ModelApi};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

mod expiration;
mod recovery;

fn wait(cx: &mut VisualTestContext, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.update(|_, cx| predicate(cx)) {
            return;
        }
        assert!(Instant::now() < deadline, "authorization update deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}
fn tap(cx: &mut VisualTestContext, selector: &'static str) {
    draw(cx);
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    draw(cx);
}
fn settle_dialog(cx: &mut VisualTestContext) {
    draw(cx);
    std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
    draw(cx);
}

struct Surface(Entity<Workspace>);
impl Render for Surface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

struct Fixture {
    runtime: Arc<tokio::runtime::Runtime>,
    node: Node,
    controller: Link,
    transport: Arc<dyn Transport>,
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
        let transport = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        Self {
            runtime,
            node,
            controller,
            transport,
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
    fn provider(&self) -> Provider {
        let Output::Providers(providers) = self.execute(Command::ListProviders) else {
            panic!("providers expected")
        };
        providers[0].clone()
    }
    fn close(self) {
        self.runtime.block_on(self.controller.close()).unwrap();
        self.runtime.block_on(self.node.shutdown()).unwrap();
    }
}

#[gpui::test]
fn authorization_lifecycle(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for (authentication, api) in [
        (Authentication::ChatGpt, ModelApi::Responses),
        (Authentication::Copilot, ModelApi::Responses),
        (Authentication::Copilot, ModelApi::ChatCompletions),
    ] {
        for remote in [false, true] {
            let fixture = Fixture::new(remote);
            fixture.execute(Command::SaveProvider {
                provider: support::provider(authentication, api),
                expected_revision: 0,
                secret: None,
            });
            let mut owner = None;
            let (_, visual) = cx.add_window_view(|window, cx| {
                let view = cx.new(|cx| Workspace::new(window, cx));
                view.update(cx, |view, cx| {
                    view.bind_providers(
                        fixture.transport.clone(),
                        fixture.runtime.clone(),
                        "Authorization Node".into(),
                        cx,
                    );
                    view.select(crate::settings::Section::Providers, cx);
                });
                owner = Some(view.clone());
                let surface = cx.new(|_| Surface(view));
                Root::new(surface, window, cx)
            });
            let owner = owner.unwrap();
            wait(visual, |cx| owner.read(cx).providers.channels.len() == 1);
            draw(visual);
            let kind = visual.debug_bounds("provider-kind-0").unwrap();
            let count = visual.debug_bounds("provider-count-0").unwrap();
            assert_eq!(kind.top(), count.top());
            assert!(
                (count.left() - kind.right() - visual.update(|window, _| window.rem_size() * 0.75))
                    .abs()
                    < px(1.)
            );
            assert_eq!(
                visual.debug_bounds("provider-login-0").unwrap().size,
                visual.debug_bounds("provider-edit-0").unwrap().size
            );
            let login = visual.update(|window, cx| open(owner.clone(), 0, window, cx).unwrap());
            settle_dialog(visual);
            wait(visual, |cx| {
                matches!(login.read(cx).state(), Some(State::Pending { .. }))
            });
            settle_dialog(visual);
            let card = visual.debug_bounds("provider-login-code").unwrap();
            let text = visual.debug_bounds("provider-login-code-text").unwrap();
            assert!((card.center().x - text.center().x).abs() < px(1.));
            assert!((card.center().y - text.center().y).abs() < px(1.));
            assert!(visual.debug_bounds("provider-login-status").is_none());
            assert!(visual.debug_bounds("provider-login-copy").is_none());
            let footer = visual.debug_bounds("provider-login-footer").unwrap();
            let authorize = visual.debug_bounds("provider-login-open").unwrap();
            let cancel = visual.debug_bounds("provider-login-cancel").unwrap();
            assert!(card.bottom() < footer.top());
            assert!((cancel.right() - footer.right()).abs() < px(1.));
            assert!(authorize.right() < cancel.left());
            assert_eq!(authorize.top(), cancel.top());
            tap(visual, "provider-login-code");
            visual.update(|_, cx| {
                assert_eq!(
                    cx.read_from_clipboard().unwrap().text().as_deref(),
                    Some("SAIL-1234")
                )
            });
            crate::feedback::tests::shown(visual);
            assert_eq!(
                visual.update(crate::feedback::tests::summary),
                tr("content_copied")
            );
            visual.simulate_keystrokes("escape");
            wait(visual, |cx| {
                login.read(cx).closed || matches!(login.read(cx).state(), Some(State::Cancelled))
            });
            crate::feedback::tests::shown(visual);
            assert_eq!(
                visual.update(crate::feedback::tests::summary),
                tr("provider_login_cancelled")
            );
            draw(visual);
            assert!(visual.debug_bounds("provider-login-status").is_none());
            // Cancellation acknowledgement leaves a stable completion surface.
            tap(visual, "provider-login-cancel");
            visual.update(|window, cx| assert!(!window.has_active_dialog(cx)));
            assert!(fixture.provider().credential.is_none());
            visual.update(|window, cx| window.clear_notifications(cx));
            fixture.mode.store(1, Ordering::SeqCst);
            let login = visual.update(|window, cx| open(owner.clone(), 0, window, cx).unwrap());
            settle_dialog(visual);
            wait(visual, |cx| {
                matches!(login.read(cx).state(), Some(State::Connected))
            });
            wait(visual, |cx| {
                owner.read(cx).providers.channels[0].credential_configured
            });
            crate::feedback::tests::shown(visual);
            assert_eq!(
                visual.update(crate::feedback::tests::summary),
                tr("provider_login_connected")
            );
            draw(visual);
            assert!(visual.debug_bounds("provider-login-status").is_none());
            let original = visual.update(|window, cx| window.notifications(cx));
            visual.update(|window, cx| {
                login.update(cx, |login, cx| {
                    login.report("provider_login_connected", window, cx)
                });
                assert_eq!(window.notifications(cx), original);
            });
            tap(visual, "provider-login-cancel");
            visual.update(|window, cx| assert!(!window.has_active_dialog(cx)));
            tap(visual, "provider-logout-0");
            settle_dialog(visual);
            crate::prompts::tests::answer(visual, "settings_cancel");
            let Output::Credentials(credentials) = fixture.execute(Command::ListCredentials) else {
                panic!("credentials expected")
            };
            assert!(!credentials[0].revoked);
            tap(visual, "provider-logout-0");
            settle_dialog(visual);
            crate::prompts::tests::answer(visual, "provider_disconnect");
            wait(visual, |_| {
                match fixture.execute(Command::ListCredentials) {
                    Output::Credentials(credentials) => credentials[0].revoked,
                    _ => false,
                }
            });
            draw(visual);
            let reference = fixture.provider().credential.unwrap();
            assert_eq!(reference.id, credentials[0].id);
            assert!(!visual.has_pending_prompt());
            tap(visual, "provider-login-0");
            settle_dialog(visual);
            wait(visual, |_| {
                match fixture.execute(Command::ListCredentials) {
                    Output::Credentials(credentials) => !credentials[0].revoked,
                    _ => false,
                }
            });
            assert_eq!(fixture.provider().credential.unwrap(), reference);
            assert_eq!(
                fixture
                    .server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|request| matches!(
                        request.path.as_str(),
                        "/api/accounts/deviceauth/usercode" | "/login/device/code"
                    ))
                    .count(),
                3
            );
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}
