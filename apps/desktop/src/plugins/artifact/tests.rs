use super::*;
use crate::conversation::live::tests::{fixture as chat, wait};
use crate::plugins::tests::{init, snapshot};
use core::prelude::v1::test;
use gpui_kit::{
    component::{Root, WindowExt},
    test::TestWindowExt,
};
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

mod shell;

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

struct Document(Entity<Panel>);

fn frame(panel: &Entity<Panel>, cx: &App) -> AnyElement {
    gpui_kit::component::v_flex()
        .size_full()
        .child(
            gpui_kit::component::h_flex()
                .h(px(crate::preview::HEADER_HEIGHT))
                .child(Panel::heading(panel, cx))
                .children(panel.read(cx).header()),
        )
        .child(div().flex_1().min_h_0().child(panel.clone()))
        .into_any_element()
}

impl Render for Document {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        frame(&self.0, cx)
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct Answer {
    source: Entity<Chat>,
    panel: Entity<Panel>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Render for Answer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::component::h_flex()
            .size_full()
            .child(div().flex_1().min_w_0().h_full().child(self.source.clone()))
            .child(div().w(px(480.)).h_full().child(frame(&self.panel, cx)))
    }
}

#[gpui_kit::test]
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn clears_admitted_answers_after_surface_close(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = chat::Fixture::with_server(remote, |runtime| {
            runtime.block_on(crate::agent_fixture::Server::scripted_tools(vec![(
                crate::agent_fixture::plugin_tool(PACKAGE, "write_brief"),
                write(None, None),
            )]))
        });
        install(&fixture);
        fixture.start();
        let first = published(&fixture, 1);
        fixture.server.replace_tools(Vec::new());
        let observed = Arc::new(AdmissionLoss {
            inner: fixture.transport.clone(),
            first: AtomicBool::new(true),
            requests: Mutex::new(Vec::new()),
        });
        fixture.binding.client = Arc::new(sailry_client::Client::new(observed.clone()));
        #[cfg(all(target_os = "macos", feature = "workload-tests"))]
        cx.update(|cx| cx.set_global(native_flow::Surfaces::default()));
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
            let panel = cx.new(|cx| Panel::document(source.clone(), first.clone(), cx));
            owner = Some((source.clone(), panel.clone()));
            let harness = cx.new(|_| Answer { source, panel });
            Root::new(harness, window, cx)
        });
        let (source, panel) = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1280.), px(900.)));
        version(&panel, visual, 1);
        wait(visual, |cx| {
            source.read(cx).document_writable(first.session)
        });
        let resource = panel.read_with(visual, |panel, _| panel.document.clone().unwrap());
        let host = panel.read_with(visual, |panel, _| panel.mounted.as_ref().unwrap().host());
        #[cfg(all(target_os = "macos", feature = "workload-tests"))]
        {
            let key = (resource.binding.client.target(), resource.artifact.id);
            let owner = visual.read(|cx| {
                cx.global::<native_flow::Surfaces>()
                    .0
                    .get(&key)
                    .and_then(WeakEntity::upgrade)
                    .unwrap()
            });
            surface::tests::settle_mount(owner, resource.artifact.revision, visual);
        }
        let draft = surface::tests::Draft::new(resource, host.clone(), visual);
        crate::feedback::tests::settle(visual);
        visual.update(|window, cx| window.clear_notifications(cx));
        draft.answer(visual);
        crate::feedback::tests::shown(visual);
        visual.update(|window, cx| {
            assert_eq!(
                crate::feedback::tests::summary(window, cx),
                crate::tr("chat_action_unknown")
            )
        });
        let cache = draft.close();
        let node = fixture.binding.client.target();
        visual.update(|_, cx| {
            assert_eq!(
                crate::preferences::artifacts::drafts(node, first.id, 1, cx).len(),
                1
            )
        });
        chat::tap(visual, "chat-retry");
        wait(visual, |cx| {
            crate::preferences::artifacts::drafts(node, first.id, 1, cx).is_empty()
        });
        visual.update(|_, cx| {
            cx.set_global(crate::preferences::Preferences::open(
                cache.path().join("preferences.json"),
            ));
            assert!(crate::preferences::artifacts::drafts(node, first.id, 1, cx).is_empty());
        });
        let requests = observed.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], requests[1]);
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(source);
        host.close();
        drop(host);
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
        expected_digest: None,
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
    let expected = revision.to_string();
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        let (selected, rendered, state, notices) = visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            let rendered = snapshot(panel, cx);
            let state = panel.read(cx);
            (
                state.header().and_then(|header| {
                    let header = header.read(cx);
                    header
                        .filter_ready("brief-version", cx)
                        .then(|| header.filter_selected("brief-version").map(str::to_owned))
                        .flatten()
                }),
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
        if selected.as_deref() == Some(expected.as_str())
            && visual.debug_bounds("brief-version").is_some()
        {
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
            let harness = cx.new(|_| Document(panel));
            Root::new(harness, window, cx)
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
        // The verified readonly toast covers this harness's centered Header.
        visual.update(|window, cx| window.clear_notifications(cx));
        crate::feedback::tests::settle(visual);
        visual.update(|window, cx| {
            window.render_frame(cx);
            let header = panel.read(cx).header().unwrap();
            let id = header.read(cx).filter_id("brief-version").unwrap();
            window.within(id).click("input", cx);
        });
        visual.run_until_parked();
        assert!(visual.debug_bounds("list-search").is_none());
        visual.update(|window, cx| {
            window.render_frame(cx);
            let header = panel.read(cx).header().unwrap();
            let interaction = header
                .read(cx)
                .filter_interaction("brief-version", window, cx);
            assert!(
                gpui_kit::component::GlobalState::is_in_deferred_context(cx),
                "version picker must open after its trigger click: {interaction:?}"
            );
            assert_eq!(
                interaction,
                Some((Some(0), Some("2".into()), true)),
                "version picker must focus its current choice"
            );
        });
        visual.update(|window, cx| window.press("down", cx));
        visual.run_until_parked();
        visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            let header = panel.read(cx).header().unwrap();
            assert_eq!(
                header
                    .read(cx)
                    .filter_interaction("brief-version", window, cx),
                Some((Some(1), Some("2".into()), true)),
                "the down key must move the native version cursor"
            );
        });
        visual.update(|window, cx| window.press("enter", cx));
        visual.run_until_parked();
        visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            let header = panel.read(cx).header().unwrap();
            let interaction = header
                .read(cx)
                .filter_interaction("brief-version", window, cx);
            assert_eq!(
                interaction.as_ref().and_then(|state| state.1.as_deref()),
                Some("1"),
                "the enter key must commit the native version choice: {interaction:?}"
            );
            assert!(
                !gpui_kit::component::GlobalState::is_in_deferred_context(cx),
                "the confirmed version picker must close"
            );
        });
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
            let harness = cx.new(|_| Document(panel));
            Root::new(harness, window, cx)
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
        assert!(captured(&Some(resource.clone()), first.id, &host).is_ok());
        let native = visual
            .update(|_, cx| {
                export::read_card(
                    &resource,
                    &export::Options {
                        artifact: first.id,
                        revision: 1,
                        format: export::Format::Html,
                    },
                    cx,
                )
            })
            .unwrap();
        let native = fixture.runtime.block_on(native).unwrap();
        assert_eq!(native.artifact, first);
        assert_eq!(native.latest_revision, 1);
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
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let retained = surface::tests::Draft::new(
            panel.read_with(visual, |panel, _| panel.document.clone().unwrap()),
            host.clone(),
            visual,
        );
        let Output::Conversation(before) = fixture.execute(Command::ReadConversation {
            session: fork_id,
            before: None,
            limit: 100,
        }) else {
            panic!("conversation expected");
        };
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            visual.update(|window, cx| window.clear_notifications(cx));
            retained.answer(visual);
            crate::feedback::tests::shown(visual);
            visual.update(|window, cx| {
                assert_eq!(
                    crate::feedback::tests::summary(window, cx),
                    crate::tr("chat_action_failed")
                );
            });
            let Output::Conversation(rejected) = fixture.execute(Command::ReadConversation {
                session: fork_id,
                before: None,
                limit: 100,
            }) else {
                panic!("conversation expected");
            };
            assert_eq!(rejected.page.runs, before.page.runs);
        }
        let directory = fixture
            .node
            .profile()
            .join("artifacts")
            .join(first.session.to_string());
        assert!(directory.is_dir());
        let Output::Session(original) = fixture.execute(Command::ReadSession {
            session: first.session,
        }) else {
            panic!("session expected");
        };
        fixture.execute(Command::RemoveSession {
            session: original.id,
            expected_revision: original.revision,
        });
        assert!(directory.is_dir());
        let retained_content = fixture
            .runtime
            .block_on(
                host.read_artifact(first.id, Some(1), first.session)
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(retained_content.html, content.html);
        assert_eq!(retained_content.view, content.view);
        visual.update(|window, cx| window.clear_notifications(cx));
        fixture.execute(Command::RemovePlugin {
            name: package.summary.name.clone(),
            expected_revision: package.summary.revision,
        });
        wait(visual, |cx| {
            panel.read(cx).mounted.is_none() && panel.read(cx).error == Some("plugins_uninstalled")
        });
        crate::feedback::tests::shown(visual);
        visual.update(|window, cx| {
            assert_eq!(
                crate::feedback::tests::summary(window, cx),
                crate::tr("plugins_uninstalled")
            )
        });
        assert!(host.check().is_err());
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            retained.answer(visual);
            drop(retained.close());
        }
        let Output::Conversation(after) = fixture.execute(Command::ReadConversation {
            session: fork_id,
            before: None,
            limit: 100,
        }) else {
            panic!("conversation expected");
        };
        assert_eq!(after.page.runs, before.page.runs);
        let frozen = fixture
            .runtime
            .block_on(
                fixture
                    .binding
                    .client
                    .read_artifact_in(fork_id, first.id, Some(1)),
            )
            .unwrap();
        assert_eq!(frozen.html, content.html);
        assert_eq!(frozen.view, content.view);
        let native = visual
            .update(|_, cx| {
                export::read_card(
                    &resource,
                    &export::Options {
                        artifact: first.id,
                        revision: 1,
                        format: export::Format::Html,
                    },
                    cx,
                )
            })
            .unwrap();
        let native = fixture.runtime.block_on(native).unwrap();
        assert_eq!(native.html, frozen.html);
        assert_eq!(native.view, frozen.view);
        let session = source.read_with(visual, |source, _| source.summary().unwrap().clone());
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(source);
        host.close();
        drop(host);
        let mut reopened = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let source = cx.new(|cx| Chat::new(fixture.binding.clone(), Some(session), window, cx));
            let panel = cx.new(|cx| Panel::document(source, first.clone(), cx));
            reopened = Some(panel.clone());
            let harness = cx.new(|_| Document(panel));
            Root::new(harness, window, cx)
        });
        let panel = reopened.unwrap();
        wait(visual, |cx| {
            panel.read(cx).error == Some("plugins_uninstalled") && panel.read(cx).mounted.is_none()
        });
        crate::feedback::tests::shown(visual);
        visual.update(|window, cx| {
            assert_eq!(
                crate::feedback::tests::summary(window, cx),
                crate::tr("plugins_uninstalled")
            )
        });
        visual.update(|window, _| window.remove_window());
        drop(panel);
        let Output::Session(fork) = fixture.execute(Command::ReadSession { session: fork_id })
        else {
            panic!("session expected");
        };
        fixture.execute(Command::RemoveSession {
            session: fork.id,
            expected_revision: fork.revision,
        });
        assert!(!directory.exists());
        assert_eq!(
            fixture
                .runtime
                .block_on(
                    fixture
                        .binding
                        .client
                        .read_artifact_in(fork_id, first.id, Some(1))
                )
                .unwrap_err()
                .code,
            sailry_protocol::ErrorCode::NotFound
        );
        fixture.close();
    }
}

#[test]
fn includes_raw_choices_and_document_identity() {
    rust_i18n::set_locale("en");
    let content = Content {
        artifact: Summary {
            kind: sailry_protocol::artifact::Kind::Document,
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

mod cards {
    use super::*;
    use sailry_link::CancellationToken;
    use sailry_protocol::artifact::Kind;
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::AtomicUsize;

    struct Reads {
        inner: Arc<dyn Transport>,
        target: NodeId,
        mode: AtomicUsize,
        hold: AtomicBool,
        entered: CancellationToken,
        release: CancellationToken,
        requests: Mutex<Vec<Request>>,
    }

    impl Transport for Reads {
        fn target(&self) -> NodeId {
            self.target
        }
        fn open(&self, id: StreamId) -> Pending<'_, Result<Stream, Fault>> {
            self.inner.open(id)
        }
        fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
            self.inner.subscribe(topic)
        }
        fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
            Box::pin(async move {
                let read = matches!(request.command, Command::ReadArtifact { .. });
                if read {
                    self.requests.lock().unwrap().push(request.clone());
                }
                let admitted = self.inner.dispatch(request).await?;
                if read && self.hold.load(Ordering::SeqCst) {
                    self.entered.cancel();
                    self.release.cancelled().await;
                }
                let mode = self.mode.load(Ordering::SeqCst);
                if !read || mode == 0 {
                    return Ok(admitted);
                }
                let mut output = admitted.completion.await.unwrap()?;
                let Output::Artifact(content) = &mut output else {
                    panic!("artifact expected");
                };
                match mode {
                    1 => content.artifact.session = sailry_protocol::SessionId::new(),
                    2 => {
                        content.artifact.kind = if content.artifact.kind == Kind::Document {
                            Kind::Preview
                        } else {
                            Kind::Document
                        }
                    }
                    3 => content.artifact.package.digest = "0".repeat(64),
                    4 => content.artifact.id = ArtifactId::new(),
                    5 => {
                        content.artifact.revision += 1;
                        content.latest_revision = content.artifact.revision;
                    }
                    _ => unreachable!(),
                }
                let (sender, completion) = tokio::sync::oneshot::channel();
                sender.send(Ok(output)).unwrap();
                Ok(Admission {
                    receipt: admitted.receipt,
                    completion,
                })
            })
        }
    }

    fn fixture(remote: bool, kind: Kind) -> (chat::Fixture, Arc<Reads>) {
        let mut draft = write(None, None);
        draft["kind"] = json!(kind);
        if kind == Kind::Preview {
            draft["html"] = json!(
                "<!doctype html><html><head></head><body><h1>Native preview</h1></body></html>"
            );
        }
        let mut fixture = chat::Fixture::with_server(remote, |runtime| {
            runtime.block_on(crate::agent_fixture::Server::scripted_tools(vec![(
                crate::agent_fixture::plugin_tool(PACKAGE, "write_brief"),
                draft,
            )]))
        });
        install(&fixture);
        fixture.start();
        let observed = Arc::new(Reads {
            inner: fixture.transport.clone(),
            target: fixture.transport.target(),
            mode: AtomicUsize::new(0),
            hold: AtomicBool::new(false),
            entered: CancellationToken::new(),
            release: CancellationToken::new(),
            requests: Mutex::new(Vec::new()),
        });
        fixture.binding.client = Arc::new(sailry_client::Client::new(observed.clone()));
        (fixture, observed)
    }

    fn resource(source: &Entity<Chat>, artifact: &Summary, cx: &App) -> Resource {
        Resource {
            binding: source.read(cx).binding(),
            session: source.read(cx).session().unwrap(),
            artifact: artifact.clone(),
            source: source.downgrade(),
        }
    }

    fn options(artifact: &Summary) -> export::Options {
        export::Options {
            artifact: artifact.id,
            revision: artifact.revision,
            format: export::Format::Html,
        }
    }

    fn choose(
        resource: Resource,
        visual: &mut VisualTestContext,
    ) -> tokio::sync::oneshot::Receiver<bool> {
        let (sender, mut receiver) = tokio::sync::oneshot::channel();
        visual.update(|window, cx| {
            export::choose_card(resource, export::Format::Html, sender, window, cx)
        });
        let early = receiver.try_recv();
        assert!(
            matches!(early, Err(tokio::sync::oneshot::error::TryRecvError::Empty)),
            "artifact export resolved before path picker: {early:?}"
        );
        let platform = visual.cx.clone();
        wait(visual, |_| platform.did_prompt_for_new_path());
        receiver
    }

    fn result(reply: tokio::sync::oneshot::Receiver<bool>, visual: &mut VisualTestContext) -> bool {
        let reply = RefCell::new(reply);
        let value = Cell::new(None);
        wait(visual, |_| {
            if let Ok(result) = reply.borrow_mut().try_recv() {
                value.set(Some(result));
            }
            value.get().is_some()
        });
        value.get().unwrap()
    }

    #[gpui_kit::test]
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn exports_captured_preview_and_cancels(cx: &mut TestAppContext) {
        init(cx);
        for remote in [false, true] {
            let (fixture, reads) = fixture(remote, Kind::Preview);
            let first = published(&fixture, 1);
            let (source, visual) = chat::open(cx, fixture.binding.clone(), fixture.session.clone());
            wait(visual, |cx| {
                let source = source.read(cx);
                source.connected() && source.document_visible(&first)
            });
            visual.update(|window, cx| source.update(cx, |source, cx| source.focus(window, cx)));
            visual.simulate_input("Keep the existing draft");
            let captured = visual.update(|_, cx| resource(&source, &first, cx));
            let model_requests = fixture.server.requests.lock().unwrap().len();
            let cancelled = choose(captured.clone(), visual);
            visual.simulate_new_path_selection(|_| None);
            assert!(!result(cancelled, visual));
            assert!(reads.requests.lock().unwrap().is_empty());

            let destination = fixture.directory.path().join("native-preview.html");
            let saved = choose(captured.clone(), visual);
            visual.simulate_new_path_selection(|_| Some(destination.clone()));
            assert!(result(saved, visual));
            let html = std::fs::read_to_string(&destination).unwrap();
            assert!(html.contains("Native preview"));
            assert!(html.contains("sandbox=\"allow-scripts\""));
            assert!(!html.contains("window.sailryDocument"));
            assert_eq!(reads.requests.lock().unwrap().len(), 1);

            reads.mode.store(1, Ordering::SeqCst);
            let rejected_path = fixture.directory.path().join("foreign-preview.html");
            let rejected = choose(captured.clone(), visual);
            visual.simulate_new_path_selection(|_| Some(rejected_path.clone()));
            assert!(!result(rejected, visual));
            assert!(!rejected_path.exists());
            assert_eq!(reads.requests.lock().unwrap().len(), 2);
            assert_eq!(
                fixture.server.requests.lock().unwrap().len(),
                model_requests
            );
            assert_eq!(
                source.read_with(visual, |source, cx| source.draft(cx).to_string()),
                "Keep the existing draft"
            );
            assert_eq!(
                source.read_with(visual, |source, _| source.session()),
                Some(first.session)
            );
            reads.mode.store(0, Ordering::SeqCst);

            reads.hold.store(true, Ordering::SeqCst);
            let held_path = fixture.directory.path().join("held-source.html");
            let held = choose(captured, visual);
            visual.simulate_new_path_selection(|_| Some(held_path.clone()));
            wait(visual, |_| reads.entered.is_cancelled());
            visual.update(|window, cx| {
                source.update(cx, |source, cx| {
                    *source = Chat::new(fixture.binding.clone(), None, window, cx);
                    cx.notify();
                })
            });
            reads.release.cancel();
            assert!(!result(held, visual));
            assert!(!held_path.exists());
            assert_eq!(reads.requests.lock().unwrap().len(), 3);
            reads.hold.store(false, Ordering::SeqCst);
            visual.update(|window, cx| {
                source.update(cx, |source, cx| {
                    *source = Chat::new(
                        fixture.binding.clone(),
                        Some(fixture.session.clone()),
                        window,
                        cx,
                    );
                    cx.notify();
                })
            });
            wait(visual, |cx| {
                let source = source.read(cx);
                source.connected() && source.document_visible(&first)
            });
            let captured = visual.update(|_, cx| resource(&source, &first, cx));

            let changed_path = fixture.directory.path().join("changed-source.html");
            let changed = choose(captured, visual);
            visual.update(|window, cx| {
                source.update(cx, |source, cx| {
                    *source = Chat::new(fixture.binding.clone(), None, window, cx);
                    cx.notify();
                })
            });
            visual.simulate_new_path_selection(|_| Some(changed_path.clone()));
            assert!(!result(changed, visual));
            assert!(!changed_path.exists());
            assert_eq!(reads.requests.lock().unwrap().len(), 3);
            visual.update(|window, _| window.remove_window());
            drop(source);
            fixture.close();
        }
    }

    #[gpui_kit::test]
    fn rejects_changed_scope_before_reading(cx: &mut TestAppContext) {
        init(cx);
        for remote in [false, true] {
            let (fixture, reads) = fixture(remote, Kind::Document);
            let first = published(&fixture, 1);
            let (source, visual) = chat::open(cx, fixture.binding.clone(), fixture.session.clone());
            wait(visual, |cx| source.read(cx).document_visible(&first));
            let captured = visual.update(|_, cx| resource(&source, &first, cx));
            let mut changed = Vec::new();
            let mut foreign = captured.clone();
            foreign.session = sailry_protocol::SessionId::new();
            changed.push(foreign);
            let mut foreign = captured.clone();
            foreign.binding.worktree = Some(sailry_protocol::WorktreeId::new());
            changed.push(foreign);
            let mut foreign = captured.clone();
            foreign.binding.project = Some(sailry_protocol::ProjectId::new());
            changed.push(foreign);
            let mut foreign = captured.clone();
            foreign.artifact.title = "Unpublished receipt".into();
            changed.push(foreign);
            let mut foreign = captured.clone();
            let mut target = fixture.node.id();
            target.0[0] ^= 1;
            foreign.binding.client = Arc::new(sailry_client::Client::new(Arc::new(Reads {
                inner: fixture.transport.clone(),
                target,
                mode: AtomicUsize::new(0),
                hold: AtomicBool::new(false),
                entered: CancellationToken::new(),
                release: CancellationToken::new(),
                requests: Mutex::new(Vec::new()),
            })));
            changed.push(foreign);
            for changed in changed {
                assert!(
                    visual
                        .update(|_, cx| export::read_card(&changed, &options(&first), cx))
                        .is_err()
                );
            }
            for (id, revision) in [(ArtifactId::new(), 1), (first.id, 0), (first.id, 2)] {
                let options = export::Options {
                    artifact: id,
                    revision,
                    format: export::Format::Html,
                };
                assert!(
                    visual
                        .update(|_, cx| export::read_card(&captured, &options, cx))
                        .is_err()
                );
            }
            assert!(reads.requests.lock().unwrap().is_empty());
            visual.update(|window, _| window.remove_window());
            drop(source);
            cx.update(|cx| assert!(export::read_card(&captured, &options(&first), cx).is_err()));
            assert!(reads.requests.lock().unwrap().is_empty());
            fixture.close();
        }
    }

    #[gpui_kit::test]
    fn rejects_foreign_response_identity(cx: &mut TestAppContext) {
        init(cx);
        for remote in [false, true] {
            for kind in [Kind::Document, Kind::Preview] {
                let (fixture, reads) = fixture(remote, kind);
                let first = published(&fixture, 1);
                let (source, visual) =
                    chat::open(cx, fixture.binding.clone(), fixture.session.clone());
                wait(visual, |cx| source.read(cx).document_visible(&first));
                let captured = visual.update(|_, cx| resource(&source, &first, cx));
                for mode in 1..=5 {
                    reads.mode.store(mode, Ordering::SeqCst);
                    let read = visual
                        .update(|_, cx| export::read_card(&captured, &options(&first), cx))
                        .unwrap();
                    assert_eq!(
                        fixture.runtime.block_on(read).unwrap_err(),
                        "artifact_export_failed"
                    );
                }
                reads.mode.store(0, Ordering::SeqCst);
                let read = visual
                    .update(|_, cx| export::read_card(&captured, &options(&first), cx))
                    .unwrap();
                assert_eq!(fixture.runtime.block_on(read).unwrap().artifact, first);
                let requests = reads.requests.lock().unwrap();
                assert_eq!(requests.len(), 6);
                assert!(requests.iter().all(|request| matches!(request.command,
                    Command::ReadArtifact { id, revision: Some(1), session: Some(session) }
                    if id == first.id && session == fixture.session.id)));
                assert!(
                    requests
                        .iter()
                        .all(|request| request.target == fixture.node.id()
                            && request.plugin.is_none())
                );
                drop(requests);
                visual.update(|window, _| window.remove_window());
                drop(source);
                fixture.close();
            }
        }
    }
}
