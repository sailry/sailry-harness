use super::*;
use crate::conversation::live::tests::{cards as fixtures, fixture, wait};
use crate::ui::observation;
use sailry_link::{Admission, Pending, Subscription as LinkSubscription, Transport};
use sailry_protocol::{ErrorCode, Fault, NodeId, Request, RequestOutcome, Topic, Update};
use serde_json::json;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

#[path = "../../../../../../crates/node-runtime/tests/support/mcp_input.rs"]
mod input;
#[path = "../../../../../../crates/node-runtime/tests/support/mcp_oauth.rs"]
mod issuer;
#[path = "../../../../../../crates/node-runtime/tests/support/mcp.rs"]
mod mcp_peer;

#[track_caller]
fn shown(visual: &mut VisualTestContext, selector: &'static str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if visual.debug_bounds(selector).is_some() {
            return;
        }
        assert!(Instant::now() < deadline, "missing {selector}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[track_caller]
fn action(visual: &mut VisualTestContext, label: &'static str) -> Bounds<Pixels> {
    use gpui_kit::test::TestWindowExt as _;

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        let action = visual.update(|window, cx| {
            window.render_frame(cx);
            observation::try_label(window, label)
                .filter(|action| {
                    // Native accessibility reports the disabled flag only when set.
                    action.role() == Some(Role::Button)
                        && action.visible()
                        && action.disabled() != Some(true)
                })
                .map(|action| action.bounds())
        });
        if let Some(action) = action {
            return action;
        }
        assert!(Instant::now() < deadline, "missing enabled action {label}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn preserves_uncertain_requests_without_replay() {
    for code in [
        sailry_protocol::ErrorCode::OutcomeUnknown,
        sailry_protocol::ErrorCode::Unavailable,
        sailry_protocol::ErrorCode::Internal,
        sailry_protocol::ErrorCode::Busy,
        sailry_protocol::ErrorCode::Cancelled,
    ] {
        assert!(super::super::authorization::uncertain(code));
    }
    assert!(!super::super::authorization::uncertain(
        sailry_protocol::ErrorCode::PermissionDenied
    ));
    let mut login = super::super::authorization::Login::default();
    login.url = Some(("https://example.test/authorize?state=private".into(), 100));
    login.redirect = Some("http://127.0.0.1:43191/callback".into());
    login.suspend();
    assert!(login.url.is_none());
    assert!(login.redirect.is_none());
}

#[test]
fn completed_faults_are_definitive() {
    for code in [
        ErrorCode::Busy,
        ErrorCode::Unavailable,
        ErrorCode::Cancelled,
    ] {
        let fault = Fault::new(code, "isolated completed outcome");
        let (result, confirmed) = super::super::authorization::inspected(Ok(
            RequestOutcome::Completed(Box::new(Err(fault.clone()))),
        ));
        assert!(confirmed);
        assert_eq!(result.unwrap_err(), fault);
    }
    for outcome in [
        RequestOutcome::Unknown,
        RequestOutcome::Admitted,
        RequestOutcome::NotAdmitted,
    ] {
        assert!(!super::super::authorization::inspected(Ok(outcome)).1);
    }
}

#[test]
fn retains_only_live_callback_flows() {
    let mut login = super::super::authorization::Login::default();
    assert!(!login.keeps_alive());
    login.state = host::LoginState::Pending;
    login.url = Some((
        "https://example.test/authorize?state=isolated".into(),
        u64::MAX,
    ));
    assert!(!login.keeps_alive());
    let callback = sailry_client::oauth::Callback::bind("/callback").unwrap();
    login.redirect = Some(callback.redirect.clone());
    login.callback = Some(callback);
    assert!(login.can_open());
    assert!(login.keeps_alive());
    login.url.as_mut().unwrap().1 = 0;
    assert!(!login.can_open());
    assert!(!login.keeps_alive());
    login.suspend();
    assert!(!login.keeps_alive());
}

struct Loss {
    inner: Arc<dyn Transport>,
    observer: Arc<tokio::sync::Notify>,
    reject_cancel: AtomicBool,
    begin: Gate,
    cancel: Gate,
    requests: Mutex<Vec<Request>>,
}

struct Gate {
    once: AtomicBool,
    admitted: AtomicBool,
    release: tokio::sync::Notify,
}

impl Gate {
    fn new() -> Self {
        Self {
            once: AtomicBool::new(true),
            admitted: AtomicBool::new(false),
            release: tokio::sync::Notify::new(),
        }
    }
}

struct Broken {
    inner: Box<dyn LinkSubscription>,
    observer: Arc<tokio::sync::Notify>,
}

impl LinkSubscription for Broken {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            tokio::select! {
                _ = self.observer.notified() => Err(Fault::new(ErrorCode::PermissionDenied, "isolated observer ended")),
                result = self.inner.next() => result,
            }
        })
    }
}

impl Transport for Loss {
    fn open(
        &self,
        resource: sailry_protocol::StreamId,
    ) -> Pending<'_, Result<sailry_link::Stream, Fault>> {
        self.inner.open(resource)
    }

    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request.clone());
            if matches!(request.command, Command::CancelMcpLogin { .. })
                && self.reject_cancel.swap(false, Ordering::SeqCst)
            {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "isolated cancellation refused",
                ));
            }
            let gate = match &request.command {
                Command::BeginMcpLogin { .. } => Some(&self.begin),
                Command::CancelMcpLogin { .. } => Some(&self.cancel),
                _ => None,
            };
            let admission = self.inner.dispatch(request).await?;
            if let Some(gate) = gate.filter(|gate| gate.once.swap(false, Ordering::SeqCst)) {
                gate.admitted.store(true, Ordering::SeqCst);
                gate.release.notified().await;
            }
            Ok(admission)
        })
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn LinkSubscription>, Fault>> {
        Box::pin(async move {
            let login = matches!(topic, Topic::McpLogin(_));
            let stream = self.inner.subscribe(topic).await?;
            if login {
                Ok(Box::new(Broken {
                    inner: stream,
                    observer: self.observer.clone(),
                }) as Box<dyn LinkSubscription>)
            } else {
                Ok(stream)
            }
        })
    }
}

impl Loss {
    fn inspected(&self, original: &Request) -> bool {
        let mut hasher = blake3::Hasher::new();
        serde_json::to_writer(&mut hasher, original).unwrap();
        let expected = hasher.finalize().to_hex().to_string();
        self.requests.lock().unwrap().iter().any(|request| {
            matches!(&request.command, Command::InspectRequest { id, digest }
                if *id == original.id && digest == &expected)
        })
    }

    fn begins(&self) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(request.command, Command::BeginMcpLogin { .. }))
            .count()
    }
}

fn install(fixture: &fixture::Fixture, endpoint: &str) {
    let root = fixture.directory.path().join("project/card-package");
    std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
    std::fs::create_dir_all(root.join("dev.sailry.platform/desktop")).unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/settings.json"),
        json!({
            "$schema":plugin::settings::SCHEMA,
            "type":"object",
            "additionalProperties":false,
            "properties":{
                "label":{"type":"string","default":""}
            },
            "required":[]
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(root.join("dev.sailry.platform/host/main.js"), r#"
import {card,status} from 'sailry/cards';
export async function connection(){const props=await status('input');return {isError:!props.configured,error:{code:'not_configured',message:'Connect'},sailry_content:{version:1,blocks:[card('connection',props,{label:'Connect'})]}};}
"#).unwrap();
    std::fs::write(root.join("dev.sailry.platform/desktop/card.js"), r#"
import {View,div} from 'gpui-kit'; import {state,Action,nextChange} from 'sailry/card';
export default class Card extends View {
  init(_,cx){this.state=state();cx.spawn(async cx=>{let cursor=this.state.cursor;try{while(true){this.state=await nextChange(cursor);cursor=this.state.cursor;cx.notify();}}catch(_){}});}
  render(){const s=this.state;return div().v_flex().child(`${s.login}|${s.can_open}|${s.can_cancel}|${s.configured}`).child(JSON.stringify(s))
    .child(Action.new('fixture-connect',{label:s.login==='unconfirmed'||s.can_cancel?'Check':'Connect',action:{kind:'connect'}}))
    .child(Action.new('fixture-settings',{label:'Settings',action:{kind:'settings'}}))
    .children(s.can_cancel?[Action.new('fixture-cancel',{label:'Cancel',action:{kind:'cancel'}})]:[]);}
}
"#).unwrap();
    std::fs::write(
        root.join("mcp.json"),
        json!({"$schema":plugin::mcp::SCHEMA,"mcpServers":{"input":{"type":"streamable-http","url":endpoint}}}).to_string(),
    )
    .unwrap();
    std::fs::write(root.join("plugin.json"), json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"card-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{
        "api_version":"v1","actions":[],
        "settings_schema":"dev.sailry.platform/settings.json",
        "host":{"entry":"dev.sailry.platform/host/main.js","resources":["dev.sailry.platform/host/main.js"],"handlers":["connection"]},
        "desktop":{"resources":["dev.sailry.platform/desktop/card.js"]},
        "tools":[{"name":"connection","description":"Read connection","contexts":["workspace"],"presentation":"content","handler":{"name":"connection","read_only":true,"parameters":{"type":"object","properties":{},"additionalProperties":false}}}],
        "cards":[{"key":"connection","entry":"dev.sailry.platform/desktop/card.js","tool":"connection","mcp_server":"input",
            "props_schema":{"type":"object","additionalProperties":false,"properties":{"configured":{"type":"boolean"}},"required":["configured"]},
            "capabilities":{"settings":true,"browser":[{"name":"login","origins":["https://example.test"],"source":{"kind":"mcp_authorization"},"modes":["external"]}]}}]
    }}}).to_string()).unwrap();
    let Output::Plugin(package) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "card-package".into(),
        name: "card-fixture".into(),
        expected_revision: 0,
        expected_digest: None,
    }) else {
        panic!("card package expected")
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
}

fn public(card: &Card, cx: &App) -> String {
    card.mounted
        .as_ref()
        .and_then(|mounted| {
            mounted
                .root
                .read(cx)
                .content()
                .clone()
                .downcast::<gpui_shell::ScriptView>()
                .ok()
        })
        .and_then(|view| {
            view.read(cx)
                .snapshot()
                .map(|snapshot| snapshot.debug_tree())
        })
        .unwrap_or_default()
}

fn build_error(card: &Card, cx: &App) -> Option<String> {
    card.mounted
        .as_ref()
        .and_then(|mounted| {
            mounted
                .root
                .read(cx)
                .content()
                .clone()
                .downcast::<gpui_shell::ScriptView>()
                .ok()
        })
        .and_then(|view| view.read(cx).build_error().map(str::to_owned))
}

fn install_connector(fixture: &fixture::Fixture, endpoint: &str) {
    let root = fixture.directory.path().join("project/card-package");
    for (path, source) in [
        (
            "dev.sailry.platform/host/connection.js",
            include_str!("../../../../../../plugins/notion/dev.sailry.platform/host/connection.js"),
        ),
        (
            "dev.sailry.platform/desktop/connection.js",
            include_str!(
                "../../../../../../plugins/notion/dev.sailry.platform/desktop/connection.js"
            ),
        ),
        (
            "dev.sailry.platform/desktop/locales.js",
            include_str!("../../../../../../plugins/notion/dev.sailry.platform/desktop/locales.js"),
        ),
    ] {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    let mut manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../../plugins/notion/plugin.json")).unwrap();
    manifest["name"] = json!("card-fixture");
    std::fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
    let mut mcp: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../../plugins/notion/mcp.json")).unwrap();
    mcp["mcpServers"]["notion"]["url"] = json!(endpoint);
    std::fs::write(root.join("mcp.json"), mcp.to_string()).unwrap();
    let Output::Plugin(package) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "card-package".into(),
        name: "card-fixture".into(),
        expected_revision: 0,
        expected_digest: None,
    }) else {
        panic!("connection package expected")
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
}

#[gpui::test]
fn mounts_neutral_icon_cards(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![(
                crate::agent_fixture::plugin_tool("card-fixture", "connection"),
                json!({}),
            )],
        );
        let input = fixture
            .runtime
            .block_on(input::Server::with_transport(input::form_schema(), false));
        let issuer = fixture
            .runtime
            .block_on(issuer::Server::start(&input.endpoint));
        install_connector(&fixture, &issuer.endpoint);
        fixtures::submit(&fixture);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx)
                .card_instances()
                .first()
                .is_some_and(|card| card.read(cx).active(cx))
        });
        let card = view.read_with(visual, |view, _| view.card_instances()[0].clone());
        assert_eq!(
            card.read_with(visual, |card, cx| build_error(card, cx)),
            None
        );
        action(visual, "Log in");
        action(visual, "Log in externally");
        let canonical = card.read_with(visual, |card, _| card.summary.clone());
        view.read_with(visual, |view, _| {
            let history = view.tool_history();
            let snapshot = history.snapshot.as_ref().unwrap();
            let call = history
                .calls
                .iter()
                .find(|call| call.content.is_some())
                .unwrap();
            let result = call.result(&snapshot.page).unwrap();
            let output = result.get("output").unwrap_or(result);
            assert_eq!(output["configured"], json!(false));
            assert_ne!(output["isError"], json!(true));
            assert!(output.get("error").is_none());
        });
        visual.update(|window, cx| {
            window.render_frame(cx);
            let actions = observation::roles(window, Role::Button);
            assert!(
                actions
                    .iter()
                    .any(|action| action.label() == Some("Log in"))
            );
            assert!(
                actions
                    .iter()
                    .any(|action| action.label() == Some("Log in externally"))
            );
        });
        let models = fixture.task_requests();
        let refresh = action(visual, "Refresh");
        fixture::press(visual, refresh.center());
        action(visual, "Log in");
        assert_eq!(fixture.task_requests(), models);
        assert_eq!(issuer.registrations.load(Ordering::SeqCst), 0);
        assert_eq!(
            card.read_with(visual, |card, _| card.summary.clone()),
            canonical
        );
        fixture.close();
    }
}

#[gpui::test]
fn recovers_and_completes_through_native_actions(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![(
                crate::agent_fixture::plugin_tool("card-fixture", "connection"),
                json!({}),
            )],
        );
        let input = fixture
            .runtime
            .block_on(input::Server::with_transport(input::form_schema(), false));
        let issuer = fixture
            .runtime
            .block_on(issuer::Server::start(&input.endpoint));
        install(&fixture, &issuer.endpoint);
        fixtures::submit(&fixture);
        let loss = Arc::new(Loss {
            inner: fixture.transport.clone(),
            observer: Arc::new(tokio::sync::Notify::new()),
            reject_cancel: AtomicBool::new(true),
            begin: Gate::new(),
            cancel: Gate::new(),
            requests: Mutex::new(Vec::new()),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(sailry_client::Client::new(loss.clone()));
        let (view, visual) = fixture::open(cx, binding, fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx)
                .card_instances()
                .first()
                .is_some_and(|card| card.read(cx).active(cx))
        });
        let card = view.read_with(visual, |view, _| view.card_instances()[0].clone());
        let canonical = card.read_with(visual, |card, _| card.summary.clone());
        wait(visual, |_| {
            fixture
                .runtime
                .block_on(
                    fixture
                        .binding
                        .client
                        .read_conversation(fixture.session.id, None, 20),
                )
                .unwrap()
                .page
                .runs
                .iter()
                .any(|run| run.status == sailry_protocol::conversation::Status::Completed)
        });
        let models = fixture.task_requests();
        assert_eq!(loss.begins(), 0);
        assert_eq!(issuer.registrations.load(Ordering::SeqCst), 0);
        fixture::tap(visual, "fixture-settings");
        shown(visual, "plugin-settings-form");
        fixture::tap(visual, "plugin-settings-cancel");
        fixture::tap(visual, "fixture-connect");
        wait(visual, |_| {
            loss.begin.admitted.load(Ordering::SeqCst)
                && issuer.registrations.load(Ordering::SeqCst) == 1
        });
        assert_eq!(loss.begins(), 1);
        let original = loss
            .requests
            .lock()
            .unwrap()
            .iter()
            .find(|request| matches!(request.command, Command::BeginMcpLogin { .. }))
            .unwrap()
            .clone();
        card.update_in(visual, |card, _, cx| {
            card.suspend();
            cx.notify();
        });
        wait(visual, |cx| {
            card.read(cx).active(cx)
                && public(card.read(cx), cx).contains("unconfirmed|false|false|false")
        });
        assert!(card.read_with(visual, |card, _| card.authorization.attempt.is_none()));
        loss.begin.release.notify_one();
        fixture::tap(visual, "fixture-connect");
        wait(visual, |cx| {
            public(card.read(cx), cx).contains("pending|false|true|false")
        });
        let attempt = card.read_with(visual, |card, _| {
            card.authorization.attempt.clone().unwrap()
        });
        assert_eq!(attempt.id, original.id);
        assert!(loss.inspected(&original));
        assert_eq!(
            card.read_with(visual, |card, _| card
                .authorization
                .attempt
                .as_ref()
                .unwrap()
                .id),
            attempt.id
        );
        assert_eq!(loss.begins(), 1);
        assert_eq!(issuer.registrations.load(Ordering::SeqCst), 1);
        fixture::tap(visual, "fixture-cancel");
        wait(visual, |cx| {
            public(card.read(cx), cx).contains("failed|false|true|false")
        });
        fixture::tap(visual, "fixture-connect");
        wait(visual, |cx| {
            public(card.read(cx), cx).contains("pending|false|true|false")
        });
        assert_eq!(loss.begins(), 1);
        fixture::tap(visual, "fixture-cancel");
        wait(visual, |cx| {
            loss.cancel.admitted.load(Ordering::SeqCst)
                && public(card.read(cx), cx).contains("unconfirmed|false|false|false")
        });
        let original_cancel = loss
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(request.command, Command::CancelMcpLogin { .. }))
            .last()
            .unwrap()
            .clone();
        loss.cancel.release.notify_one();
        fixture::tap(visual, "fixture-connect");
        wait(visual, |cx| {
            public(card.read(cx), cx).contains("cancelled|false|false|false")
        });
        assert!(loss.inspected(&original_cancel));
        assert_eq!(loss.begins(), 1);
        fixture::tap(visual, "fixture-connect");
        wait(visual, |cx| {
            public(card.read(cx), cx).contains("pending|true|true|false")
        });
        assert_eq!(loss.begins(), 2);
        let url = card.read_with(visual, |card, _| {
            card.authorization.url.as_ref().unwrap().0.clone()
        });
        let attempt = card.read_with(visual, |card, _| {
            card.authorization.attempt.as_ref().unwrap().id
        });
        let registrations = issuer.registrations.load(Ordering::SeqCst);
        card.update_in(visual, |card, _, cx| {
            card.refresh_mount(false, cx);
            cx.notify();
        });
        assert!(card.read_with(visual, |card, cx| card.active(cx)
            && card.authorization.can_open()
            && card.authorization.attempt.as_ref().unwrap().id == attempt
            && card.authorization.url.as_ref().unwrap().0 == url));
        assert_eq!(loss.begins(), 2, "scrolling cannot restart login");
        assert!(
            !card
                .read_with(visual, |card, cx| public(card, cx))
                .contains(&url)
        );
        loss.observer.notify_one();
        wait(visual, |cx| {
            public(card.read(cx), cx).contains("unconfirmed|false|true|false")
        });
        fixture::tap(visual, "fixture-connect");
        wait(visual, |cx| {
            public(card.read(cx), cx).contains("pending|true|true|false")
        });
        assert_eq!(
            card.read_with(visual, |card, _| card
                .authorization
                .attempt
                .as_ref()
                .unwrap()
                .id),
            attempt
        );
        assert_eq!(loss.begins(), 2);
        assert_eq!(issuer.registrations.load(Ordering::SeqCst), registrations);
        let callback = fixture.runtime.block_on(issuer::Server::callback(&url));
        assert!(
            fixture
                .runtime
                .block_on(issuer::visit(&callback))
                .starts_with("HTTP/1.1 200")
        );
        wait(visual, |cx| {
            public(card.read(cx), cx).contains("connected|false|false|true")
        });
        let current = card.read_with(visual, |card, _| {
            card.mounted.as_ref().unwrap().host.package().unwrap()
        });
        assert!(current.settings_revision > canonical.package.settings_revision);
        assert_eq!(fixture.task_requests(), models);
        assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 1);
        assert_eq!(
            card.read_with(visual, |card, _| card.summary.clone()),
            canonical
        );
        let tree = card.read_with(visual, |card, cx| public(card, cx));
        for private in [
            &url,
            "access-initial",
            "refresh-initial",
            "fixture-code",
            "code_verifier",
        ] {
            assert!(!tree.contains(private));
        }
        fixture.close();
    }
}
