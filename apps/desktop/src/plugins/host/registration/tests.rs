use super::*;
use crate::plugins::{
    Panel,
    fixture::Fixture,
    tests::{click, init, snapshot, wait},
};
use core::prelude::v1::test;
use gpui_kit::component::{Root, WindowExt};
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{NodeId, Topic};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

#[path = "../../../../../../crates/node-runtime/tests/plugins/registration/server.rs"]
mod server;

fn package(fixture: &Fixture) -> plugin::Info {
    fixture.package();
    let root = fixture.directory.path().join("project/package");
    let mut manifest: Value =
        serde_json::from_str(include_str!("../../../../../../plugins/feishu/plugin.json")).unwrap();
    manifest["name"] = json!("project-summary");
    std::fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
    std::fs::write(
        root.join("mcp.json"),
        include_str!("../../../../../../plugins/feishu/mcp.json"),
    )
    .unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/settings.json"),
        include_str!("../../../../../../plugins/feishu/dev.sailry.platform/settings.json"),
    )
    .unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/desktop/settings.js"),
        include_str!("../../../../../../plugins/feishu/dev.sailry.platform/desktop/settings.js"),
    )
    .unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/desktop/locales.js"),
        include_str!("../../../../../../plugins/feishu/dev.sailry.platform/desktop/locales.js"),
    )
    .unwrap();
    fixture.install(0)
}

fn mount<'a>(
    fixture: &Fixture,
    package: &plugin::Info,
    cx: &'a mut TestAppContext,
) -> (Entity<Panel>, &'a mut VisualTestContext) {
    let mut binding = fixture.binding.clone();
    binding.project = None;
    binding.worktree = None;
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let panel = cx.new(|cx| Panel::settings(binding, false, cx));
        owner = Some(panel.clone());
        Root::new(panel, window, cx)
    });
    let panel = owner.unwrap();
    wait(visual, |cx| {
        panel.read(cx).connected && panel.read(cx).metadata.read(cx).settled()
    });
    visual.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open(package.summary.reference(), window, cx)
        })
    });
    wait(visual, |cx| {
        snapshot(&panel, cx).lines().any(|line| {
            line.contains("feishu-register") && line.contains("(\"disabled\", Bool(false))")
        })
    });
    (panel, visual)
}

fn starts(issuer: &server::Server) -> usize {
    issuer
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, body)| body.contains("action=begin"))
        .count()
}

fn script(panel: &Entity<Panel>, cx: &App) -> Entity<gpui_shell::ScriptView> {
    panel
        .read(cx)
        .mounted
        .as_ref()
        .unwrap()
        .root
        .read(cx)
        .content()
        .clone()
        .downcast::<gpui_shell::ScriptView>()
        .unwrap()
}

fn public_view(panel: &Entity<Panel>, cx: &App) {
    let text = snapshot(panel, cx);
    for marker in [
        "accounts.feishu.cn",
        "single-code",
        "private-device-marker",
        "private-app-secret-marker",
        "unused-private-user-marker",
    ] {
        assert!(
            !text.contains(marker),
            "private registration content reached the script presentation"
        );
    }
}

fn state(fixture: &Fixture, attempt: RequestId) -> State {
    fixture.runtime.block_on(async {
        let mut subscription = fixture
            .binding
            .client
            .subscribe_plugin_login(attempt)
            .await
            .unwrap();
        let sailry_protocol::Update::PluginLogin(update) =
            tokio::time::timeout(std::time::Duration::from_secs(2), subscription.next())
                .await
                .unwrap()
                .unwrap()
        else {
            panic!("registration state expected")
        };
        update.state
    })
}

fn dialog(visual: &mut VisualTestContext) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        visual
            .executor()
            .advance_clock(std::time::Duration::from_millis(10));
        visual.run_until_parked();
        if visual.update(|window, cx| window.has_active_dialog(cx)) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "configuration dialog deadline"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[gpui::test]
fn creates_only_after_click(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let issuer = fixture.runtime.block_on(server::Server::start(vec![]));
        fixture
            .node
            .set_plugin_registration_fixture(&issuer.endpoint);
        let package = package(&fixture);
        let (panel, visual) = mount(&fixture, &package, cx);
        wait(visual, |cx| {
            panel
                .read(cx)
                .mounted
                .as_ref()
                .unwrap()
                .host()
                .settings_readable()
                .is_ok()
        });
        assert_eq!(starts(&issuer), 0);
        click(visual, "feishu-register");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("Scan to create an app")
        });
        assert_eq!(starts(&issuer), 1);
        visual.update(|_, cx| public_view(&panel, cx));
        let previous = panel.read_with(visual, |_, cx| script(&panel, cx));
        issuer
            .replies
            .lock()
            .unwrap()
            .push_back(server::Server::success());
        wait(visual, |cx| snapshot(&panel, cx).contains("Configured"));
        let host = panel.read_with(visual, |panel, _| panel.mounted.as_ref().unwrap().host());
        let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
            name: package.summary.name.clone(),
        }) else {
            panic!("plugin expected")
        };
        assert_eq!(host.context().package, info.summary.reference());
        assert_eq!(
            previous,
            panel.read_with(visual, |_, cx| script(&panel, cx))
        );
        assert_eq!(starts(&issuer), 1);
        visual.update(|_, cx| public_view(&panel, cx));
        click(visual, "feishu-manual");
        dialog(visual);
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        click(visual, "plugin-settings-cancel");
        visual.update(|window, _| window.remove_window());
        host.close();
        drop(previous);
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn confirms_configured_replacement(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let issuer = fixture.runtime.block_on(server::Server::start(vec![]));
        fixture
            .node
            .set_plugin_registration_fixture(&issuer.endpoint);
        let mut package = package(&fixture);
        let Output::PluginSettings(saved) = fixture.execute(Command::SavePluginSettings {
            package: package.summary.reference(),
            values: std::collections::BTreeMap::from([("app_id".into(), json!("cli_existing"))]),
            secrets: std::collections::BTreeMap::from([(
                "app_secret".into(),
                plugin::settings::SecretUpdate::Replace(sailry_protocol::Secret::new(
                    "existing-private-marker".into(),
                )),
            )]),
        }) else {
            panic!("settings expected")
        };
        package.summary.settings_revision = saved.package.settings_revision;
        let (panel, visual) = mount(&fixture, &package, cx);
        wait(visual, |cx| snapshot(&panel, cx).contains("Replace app"));
        click(visual, "feishu-register");
        let (_, detail) = crate::prompts::tests::wait(visual);
        assert!(detail.contains(&package.summary.name));
        assert_eq!(starts(&issuer), 0);
        crate::prompts::tests::answer(visual, "settings_cancel");
        assert_eq!(starts(&issuer), 0);
        let Output::PluginSecret(secret) = fixture.execute(Command::ReadPluginSecret {
            package: saved.package.clone(),
            field: "app_secret".into(),
        }) else {
            panic!("private setting expected")
        };
        assert_eq!(secret.unwrap().expose(), "existing-private-marker");
        click(visual, "feishu-register");
        crate::prompts::tests::answer(visual, "plugin_registration_replace");
        wait(visual, |cx| {
            starts(&issuer) == 1 && snapshot(&panel, cx).contains("Scan to create an app")
        });
        let previous = panel.read_with(visual, |_, cx| script(&panel, cx));
        let attempt = fixture
            .transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .find(|request| matches!(request.command, Command::BeginPluginRegistration { .. }))
            .unwrap()
            .id;
        click(visual, "feishu-cancel");
        wait(visual, |cx| {
            let text = snapshot(&panel, cx);
            !text.contains("Scan to create an app")
                && text.lines().any(|line| {
                    line.contains("feishu-register") && line.contains("(\"disabled\", Bool(false))")
                })
        });
        assert!(matches!(state(&fixture, attempt), State::Cancelled));
        click(visual, "feishu-register");
        crate::prompts::tests::wait(visual);
        assert_eq!(starts(&issuer), 1);
        let Output::PluginSecret(secret) = fixture.execute(Command::ReadPluginSecret {
            package: saved.package.clone(),
            field: "app_secret".into(),
        }) else {
            panic!("private setting expected")
        };
        assert_eq!(secret.unwrap().expose(), "existing-private-marker");
        crate::prompts::tests::answer(visual, "plugin_registration_replace");
        wait(visual, |cx| {
            starts(&issuer) == 2 && snapshot(&panel, cx).contains("Scan to create an app")
        });
        assert_eq!(
            previous,
            panel.read_with(visual, |_, cx| script(&panel, cx))
        );
        let host = panel.read_with(visual, |panel, _| panel.mounted.as_ref().unwrap().host());
        host.close();
        visual.update(|window, _| window.remove_window());
        drop(previous);
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn refresh_and_close_cancel_owned_attempts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let issuer = fixture.runtime.block_on(server::Server::start(vec![]));
        fixture
            .node
            .set_plugin_registration_fixture(&issuer.endpoint);
        let package = package(&fixture);
        let (panel, visual) = mount(&fixture, &package, cx);
        click(visual, "feishu-register");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("Scan to create an app")
        });
        let first = fixture
            .transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .find(|request| matches!(request.command, Command::BeginPluginRegistration { .. }))
            .unwrap()
            .id;
        click(visual, "feishu-register");
        wait(visual, |cx| {
            starts(&issuer) == 2 && snapshot(&panel, cx).contains("Scan to create an app")
        });
        let previous = panel.read_with(visual, |_, cx| script(&panel, cx));
        click(visual, "feishu-cancel");
        wait(visual, |cx| {
            let text = snapshot(&panel, cx);
            !text.contains("Scan to create an app")
                && text.lines().any(|line| {
                    line.contains("feishu-register") && line.contains("(\"disabled\", Bool(false))")
                })
        });
        assert_eq!(starts(&issuer), 2);
        click(visual, "feishu-register");
        wait(visual, |cx| {
            starts(&issuer) == 3 && snapshot(&panel, cx).contains("Scan to create an app")
        });
        assert_eq!(
            previous,
            panel.read_with(visual, |_, cx| script(&panel, cx))
        );
        let requests: Vec<_> = fixture
            .transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(request.command, Command::BeginPluginRegistration { .. }))
            .cloned()
            .collect();
        assert_eq!(requests.len(), 3);
        assert_ne!(requests[0].id, requests[1].id);
        assert_ne!(requests[1].id, requests[2].id);
        assert!(matches!(state(&fixture, first), State::Cancelled));
        assert!(matches!(state(&fixture, requests[1].id), State::Cancelled));
        let host = panel.read_with(visual, |panel, _| panel.mounted.as_ref().unwrap().host());
        host.close();
        wait(visual, |_| {
            matches!(state(&fixture, requests[2].id), State::Cancelled)
        });
        assert_eq!(starts(&issuer), 3);
        visual.update(|window, _| window.remove_window());
        drop(previous);
        drop(panel);
        fixture.close();
    }
}

enum Controlled {
    Begin,
    Cancel,
}

struct LostReceipt {
    inner: Arc<dyn Transport>,
    lost: AtomicBool,
    requests: Mutex<Vec<Request>>,
    hold: Option<(CancellationToken, CancellationToken)>,
    command: Controlled,
}

impl Transport for LostReceipt {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn open(
        &self,
        stream: sailry_protocol::StreamId,
    ) -> Pending<'_, Result<sailry_link::Stream, Fault>> {
        self.inner.open(stream)
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request.clone());
            let controlled = matches!(
                (&self.command, &request.command),
                (Controlled::Begin, Command::BeginPluginRegistration { .. })
                    | (Controlled::Cancel, Command::CancelPluginLogin { .. })
            );
            let lost = controlled && !self.lost.swap(true, Ordering::SeqCst);
            if controlled {
                if let Some((entered, release)) = &self.hold {
                    entered.cancel();
                    release.cancelled().await;
                }
            }
            let admission = self.inner.dispatch(request).await?;
            if lost {
                admission.completion.await.unwrap().unwrap();
                Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "injected registration receipt loss",
                ))
            } else {
                Ok(admission)
            }
        })
    }
}

#[gpui::test]
fn checks_original_receipt_without_replay(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let issuer = fixture.runtime.block_on(server::Server::start(vec![]));
        fixture
            .node
            .set_plugin_registration_fixture(&issuer.endpoint);
        let transport = Arc::new(LostReceipt {
            inner: fixture.transport.clone(),
            lost: AtomicBool::new(false),
            requests: Mutex::new(vec![]),
            hold: None,
            command: Controlled::Begin,
        });
        fixture.binding.client = Arc::new(sailry_client::Client::new(transport.clone()));
        let package = package(&fixture);
        let (panel, visual) = mount(&fixture, &package, cx);
        click(visual, "feishu-register");
        wait(visual, |cx| snapshot(&panel, cx).contains("Check"));
        let original = transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .find(|request| matches!(request.command, Command::BeginPluginRegistration { .. }))
            .unwrap()
            .clone();
        click(visual, "feishu-register");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("Scan to create an app")
        });
        let requests = transport.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| matches!(
                    request.command,
                    Command::BeginPluginRegistration { .. }
                ))
                .count(),
            1
        );
        assert!(requests.iter().any(|request| matches!(&request.command, Command::InspectRequest { id, digest }
            if *id == original.id && *digest == blake3::hash(&serde_json::to_vec(&original).unwrap()).to_hex().to_string())));
        drop(requests);
        assert_eq!(starts(&issuer), 1);
        click(visual, "feishu-cancel");
        wait(visual, |_| {
            matches!(state(&fixture, original.id), State::Cancelled)
        });
        assert_eq!(starts(&issuer), 1);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn close_cleans_up_late_admission(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let issuer = fixture.runtime.block_on(server::Server::start(vec![]));
        fixture
            .node
            .set_plugin_registration_fixture(&issuer.endpoint);
        let (entered, release) = (CancellationToken::new(), CancellationToken::new());
        let transport = Arc::new(LostReceipt {
            inner: fixture.transport.clone(),
            lost: AtomicBool::new(true),
            requests: Mutex::new(vec![]),
            hold: Some((entered.clone(), release.clone())),
            command: Controlled::Begin,
        });
        fixture.binding.client = Arc::new(sailry_client::Client::new(transport.clone()));
        let package = package(&fixture);
        let (panel, visual) = mount(&fixture, &package, cx);
        click(visual, "feishu-register");
        wait(visual, |_| entered.is_cancelled());
        let original = transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .find(|request| matches!(request.command, Command::BeginPluginRegistration { .. }))
            .unwrap()
            .id;
        let host = panel.read_with(visual, |panel, _| panel.mounted.as_ref().unwrap().host());
        host.close();
        visual.run_until_parked();
        assert_eq!(starts(&issuer), 0);
        release.cancel();
        wait(visual, |_| {
            transport.requests.lock().unwrap().iter().any(|request|
            matches!(request.command, Command::CancelPluginLogin { attempt } if attempt == original))
        });
        assert!(matches!(state(&fixture, original), State::Cancelled));
        assert_eq!(
            transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| matches!(
                    request.command,
                    Command::BeginPluginRegistration { .. }
                ))
                .count(),
            1
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn close_preserves_cancel_identity(cx: &mut TestAppContext) {
    init(cx);
    for (remote, lost) in [false, true]
        .into_iter()
        .flat_map(|remote| [false, true].map(move |lost| (remote, lost)))
    {
        let mut fixture = Fixture::new(remote);
        let issuer = fixture.runtime.block_on(server::Server::start(vec![]));
        fixture
            .node
            .set_plugin_registration_fixture(&issuer.endpoint);
        let (entered, release) = (CancellationToken::new(), CancellationToken::new());
        let transport = Arc::new(LostReceipt {
            inner: fixture.transport.clone(),
            lost: AtomicBool::new(!lost),
            requests: Mutex::new(vec![]),
            hold: Some((entered.clone(), release.clone())),
            command: Controlled::Cancel,
        });
        fixture.binding.client = Arc::new(sailry_client::Client::new(transport.clone()));
        let package = package(&fixture);
        let (panel, visual) = mount(&fixture, &package, cx);
        click(visual, "feishu-register");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("Scan to create an app")
        });
        click(visual, "feishu-cancel");
        wait(visual, |_| entered.is_cancelled());
        let original = transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .find(|request| matches!(request.command, Command::CancelPluginLogin { .. }))
            .unwrap()
            .clone();
        let Command::CancelPluginLogin { attempt } = &original.command else {
            unreachable!()
        };
        let attempt = *attempt;
        let host = panel.read_with(visual, |panel, _| panel.mounted.as_ref().unwrap().host());
        if lost {
            release.cancel();
            wait(visual, |cx| snapshot(&panel, cx).contains("Check"));
        }
        host.close();
        visual.run_until_parked();
        release.cancel();
        wait(visual, |_| {
            matches!(state(&fixture, attempt), State::Cancelled)
        });
        if lost {
            wait(visual, |_| {
                transport.requests.lock().unwrap().iter().any(|request|
                matches!(&request.command, Command::InspectRequest { id, digest } if *id == original.id
                    && *digest == blake3::hash(&serde_json::to_vec(&original).unwrap()).to_hex().to_string()))
            });
        }
        let requests = transport.requests.lock().unwrap();
        let cancellations: Vec<_> = requests
            .iter()
            .filter(|request| matches!(request.command, Command::CancelPluginLogin { .. }))
            .collect();
        assert_eq!(cancellations, vec![&original]);
        assert_eq!(
            requests
                .iter()
                .filter(|request| matches!(
                    request.command,
                    Command::BeginPluginRegistration { .. }
                ))
                .count(),
            1
        );
        drop(requests);
        assert_eq!(starts(&issuer), 1);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
