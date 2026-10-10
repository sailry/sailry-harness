use super::*;
use crate::conversation::live::tests::{fixture as chat, wait};
use crate::plugins::tests::{init, snapshot};
use core::prelude::v1::test;
use gpui_kit::component::{Root, WindowExt};
use sailry_link::{Admission, Pending, Stream, Subscription, Transport};
use sailry_protocol::{Command, Output, plugin};
use sailry_protocol::{Fault, NodeId, Request, StreamId, Topic};
use serde_json::json;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

const PACKAGE: &str = "document-ui-fixture";

struct AdmissionLoss {
    inner: Arc<dyn Transport>,
    first: AtomicBool,
    requests: Mutex<Vec<Request>>,
}
impl Transport for AdmissionLoss {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn open(&self, id: StreamId) -> Pending<'_, Result<Stream, Fault>> {
        self.inner.open(id)
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let submission = matches!(request.command, Command::SubmitTurn { .. });
            if submission {
                self.requests.lock().unwrap().push(request.clone());
            }
            let admission = self.inner.dispatch(request).await?;
            if !submission || !self.first.swap(false, Ordering::SeqCst) {
                return Ok(admission);
            }
            assert!(admission.receipt.durable);
            let outcome = admission.completion.await.unwrap();
            assert!(outcome.is_ok());
            let (sender, completion) = tokio::sync::oneshot::channel();
            sender
                .send(Err(Fault::new(
                    sailry_protocol::ErrorCode::OutcomeUnknown,
                    "injected completion loss",
                )))
                .unwrap();
            Ok(Admission {
                receipt: admission.receipt,
                completion,
            })
        })
    }
}

#[gpui_kit::test]
fn preserves_drafts_and_recovers_admitted_answers(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = chat::Fixture::with_server(remote, |runtime| {
            runtime.block_on(crate::agent_fixture::Server::start(false))
        });
        let observed = Arc::new(AdmissionLoss {
            inner: fixture.transport.clone(),
            first: AtomicBool::new(true),
            requests: Mutex::new(Vec::new()),
        });
        fixture.binding.client = Arc::new(sailry_client::Client::new(observed.clone()));
        let (source, visual) = chat::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            source.read(cx).document_writable(fixture.session.id)
        });
        visual.update(|window, cx| source.update(cx, |source, cx| source.focus(window, cx)));
        visual.simulate_input("Keep this composer draft");
        let (sender, status) = tokio::sync::watch::channel(None);
        assert!(visual.update(
            |window, cx| source.update(cx, |source, cx| source.send_document(
                fixture.session.id,
                "Brief v1: Storage: Execution Node (node)".into(),
                sender,
                window,
                cx
            ))
        ));
        wait(visual, |cx| {
            !source.read(cx).document_writable(fixture.session.id)
        });
        wait(visual, |_| observed.requests.lock().unwrap().len() == 1);
        crate::feedback::tests::shown(visual);
        crate::feedback::tests::settle(visual);
        assert_eq!(
            *status.borrow(),
            None,
            "uncertain admission must not acknowledge the form"
        );
        assert_eq!(
            source.read_with(visual, |source, cx| source.draft(cx).to_string()),
            "Keep this composer draft"
        );
        chat::tap(visual, "chat-retry");
        wait(visual, |_| *status.borrow() == Some(true));
        wait(visual, |cx| {
            source.read(cx).document_writable(fixture.session.id)
        });
        assert_eq!(
            source.read_with(visual, |source, cx| source.draft(cx).to_string()),
            "Keep this composer draft"
        );
        let requests = observed.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0], requests[1],
            "recovery reuses the original durable request"
        );
        let Command::SubmitTurn {
            session, message, ..
        } = &requests[0].command
        else {
            unreachable!()
        };
        assert_eq!(*session, fixture.session.id);
        assert_eq!(message.text, "Brief v1: Storage: Execution Node (node)");
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(source);
        drop(observed);
        fixture.close();
    }
}

fn install(fixture: &chat::Fixture) -> plugin::Info {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/brief");
    let target = fixture.directory.path().join("project/brief");
    crate::plugins::fixture::copy_package(&source, &target);
    let manifest = target.join("plugin.json");
    let mut definition: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    definition["name"] = json!(PACKAGE);
    std::fs::write(manifest, serde_json::to_vec(&definition).unwrap()).unwrap();
    let Output::Plugin(package) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "brief".into(),
        name: PACKAGE.into(),
        expected_revision: 0,
    }) else {
        panic!("document package expected")
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    package
}

fn write(id: Option<ArtifactId>, revision: Option<u32>) -> serde_json::Value {
    json!({"id":id,"expected_revision":revision,"title":"Architecture","summary":"Captured resources",
        "html":"<section data-title='Options'><h1>Architecture</h1><form data-name='choices'><fieldset data-name='storage'><legend>Storage</legend><label><input type='radio' name='storage' value='node'>Execution Node</label></fieldset><button type='submit'>Send</button></form></section>"})
}

fn published(fixture: &chat::Fixture, revision: u32) -> Summary {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let artifacts = fixture
            .runtime
            .block_on(
                fixture
                    .binding
                    .client
                    .list_artifacts(fixture.session.id, None, 64),
            )
            .unwrap();
        if let Some(artifact) = artifacts
            .items
            .into_iter()
            .find(|artifact| artifact.revision == revision)
        {
            return artifact;
        }
        assert!(Instant::now() < deadline, "artifact publication deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[track_caller]
fn version(panel: &Entity<Panel>, visual: &mut VisualTestContext, revision: u32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let expected = format!("(\"selected\", Str(\"{revision}\"))");
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        let (rendered, state, notices) = visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            let rendered = snapshot(panel, cx);
            let state = panel.read(cx);
            (
                rendered,
                (
                    state.connected,
                    state.loading,
                    state.selected.clone(),
                    state.metadata.read(cx).settled(),
                    state.source_error.clone(),
                ),
                (!window.notifications(cx).is_empty())
                    .then(|| crate::feedback::tests::summary(window, cx)),
            )
        });
        if rendered.lines().any(|line| {
            line.contains("SelectField")
                && line.contains("\"brief-version\"")
                && line.contains(&expected)
        }) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "document version {revision} deadline; panel (connected, loading, selected, metadata, error): {state:?}; notices: {notices:?}; snapshot:\n{rendered}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui_kit::test]
fn mounts_package_and_reads_captured_versions(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = chat::Fixture::with_server(remote, |runtime| {
            runtime.block_on(crate::agent_fixture::Server::scripted_tools(vec![(
                crate::agent_fixture::plugin_tool(PACKAGE, "write_brief"),
                write(None, None),
            )]))
        });
        let package = install(&fixture);
        fixture.start();
        let first = published(&fixture, 1);
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let source = cx.new(|cx| {
                Chat::new(
                    fixture.binding.clone(),
                    Some(fixture.session.clone()),
                    window,
                    cx,
                )
            });
            let panel = cx.new(|cx| Panel::document(source, first.clone(), cx));
            owner = Some(panel.clone());
            Root::new(panel, window, cx)
        });
        let panel = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(960.), px(680.)));
        version(&panel, visual, 1);
        let source = panel.read_with(visual, |panel, _| panel.source.clone().unwrap());
        wait(visual, |cx| source.read(cx).document_visible(&first));
        let resource = panel.read_with(visual, |panel, _| panel.document.clone().unwrap());
        let host = panel.read_with(visual, |panel, _| panel.mounted.as_ref().unwrap().host());
        assert_eq!(host.context().session, Some(first.session));
        assert_eq!(host.target(), fixture.binding.client.target());
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.selected.clone()),
            Some(package.summary.reference())
        );
        assert!(captured(&Some(resource.clone()), first.id, &host).is_ok());
        assert!(captured(&Some(resource.clone()), ArtifactId::new(), &host).is_err());
        let original = fixture
            .runtime
            .block_on(
                host.read_artifact(first.id, Some(1), first.session)
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(original.artifact.revision, 1);
        assert_eq!(original.latest_revision, 1);
        assert!(
            original
                .view
                .as_ref()
                .unwrap()
                .scripts
                .iter()
                .any(|source| source.contains("sailryDocument"))
        );
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let draft = surface::tests::Draft::new(resource, host.clone(), visual);

        fixture.server.replace_tools(vec![(
            crate::agent_fixture::plugin_tool(PACKAGE, "write_brief"),
            write(Some(first.id), Some(1)),
        )]);
        wait(visual, |cx| {
            source.read(cx).document_writable(first.session)
        });
        let (sender, status) = tokio::sync::watch::channel(None);
        assert!(visual.update(
            |window, cx| source.update(cx, |source, cx| source.send_document(
                first.session,
                "Revise the document".into(),
                sender,
                window,
                cx
            ))
        ));
        wait(visual, |_| *status.borrow() == Some(true));
        let second = published(&fixture, 2);
        assert_eq!(second.id, first.id);
        let historic = fixture
            .runtime
            .block_on(
                host.read_artifact(first.id, Some(1), first.session)
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(historic.artifact.revision, 1);
        assert_eq!(historic.latest_revision, 2);
        let latest = fixture
            .runtime
            .block_on(host.read_artifact(first.id, None, first.session).unwrap())
            .unwrap();
        assert_eq!(latest.artifact.revision, 2);
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        draft.verify(2, visual);
        version(&panel, visual, 2);
        crate::plugins::tests::click(visual, "brief-version");
        visual.simulate_keystrokes("down");
        visual.simulate_keystrokes("enter");
        version(&panel, visual, 1);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(source);
        host.close();
        drop(host);

        let Output::Session(fork) = fixture.execute(Command::ForkConversation {
            session: first.session,
            through: first.turn,
            expected_revision: fixture.session.revision,
        }) else {
            panic!("forked session expected")
        };
        let fork_id = fork.id;
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let source = cx.new(|cx| Chat::new(fixture.binding.clone(), Some(fork), window, cx));
            let panel = cx.new(|cx| Panel::document(source, first.clone(), cx));
            owner = Some(panel.clone());
            Root::new(panel, window, cx)
        });
        let panel = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(960.), px(680.)));
        version(&panel, visual, 1);
        let source = panel.read_with(visual, |panel, _| panel.source.clone().unwrap());
        wait(visual, |cx| source.read(cx).document_visible(&first));
        assert!(!source.read_with(visual, |source, _| source.document_visible(&second)));
        let mut altered = first.clone();
        altered.title = "Unpublished receipt".into();
        assert!(!source.read_with(visual, |source, _| source.document_visible(&altered)));
        assert!(!source.read_with(visual, |source, _| source.document_writable(first.session)));
        let resource = panel.read_with(visual, |panel, _| panel.document.clone().unwrap());
        let host = panel.read_with(visual, |panel, _| panel.mounted.as_ref().unwrap().host());
        assert_eq!(resource.session, fork_id);
        assert_eq!(host.context().session, Some(fork_id));
        assert!(captured(&Some(resource), first.id, &host).is_ok());
        let content = fixture
            .runtime
            .block_on(host.read_artifact(first.id, None, first.session).unwrap())
            .unwrap();
        assert_eq!(content.artifact.revision, 1);
        assert_eq!(content.latest_revision, 1);
        assert!(
            fixture
                .runtime
                .block_on(
                    host.read_artifact(first.id, Some(2), first.session)
                        .unwrap()
                )
                .is_err()
        );
        let revisions = fixture
            .runtime
            .block_on(host.artifact_revisions(first.id, None, 64).unwrap())
            .unwrap();
        assert_eq!(
            decode(&revisions).unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let (sender, status) = tokio::sync::watch::channel(None);
        assert!(!visual.update(|window, cx| source.update(cx, |source, cx| {
            source.send_document(first.session, "Inherited answer".into(), sender, window, cx)
        })));
        assert_eq!(*status.borrow(), None);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(source);
        host.close();
        drop(host);
        fixture.close();
    }
}

#[test]
fn includes_raw_choices_and_document_identity() {
    rust_i18n::set_locale("en");
    let content = Content {
        artifact: Summary {
            id: ArtifactId::new(),
            session: sailry_protocol::SessionId::new(),
            turn: sailry_protocol::TurnId::new(),
            package: plugin::Reference {
                name: "fixture".into(),
                digest: "frozen".into(),
                settings_revision: 0,
            },
            title: "Architecture".into(),
            summary: String::new(),
            revision: 3,
            size: 10,
            created_at: 1,
            updated_at: 1,
        },
        latest_revision: 3,
        html: "<p>Plan</p>".into(),
        warnings: vec![],
        view: None,
    };
    let event = json!({"form":"choices","fields":[{"name":"storage","label":"Storage","values":[{"value":"execution-node","label":"Node"}]}]});
    let message = surface::form_message(&content, &event).unwrap();
    assert!(message.contains(&content.artifact.id.to_string()));
    assert!(message.contains("Architecture"));
    assert!(message.contains("Node (execution-node)"));
    let mut invalid = event.clone();
    invalid["fields"]
        .as_array_mut()
        .unwrap()
        .push(event["fields"][0].clone());
    assert!(surface::form_message(&content, &invalid).is_err());
}
