use super::*;
use crate::plugins::fixture::code as network;
use sailry_node_runtime::{EnvironmentFixture, Node};
use std::sync::atomic::Ordering;

fn managed(remote: bool, network: &network::Network) -> (Fixture, Arc<observed::Observed>) {
    let mut fixture = fixture(remote, Permission::Ask);
    let root = fixture.directory.path().canonicalize().unwrap();
    fixture.runtime.block_on(fixture.node.shutdown()).unwrap();
    std::fs::remove_file(root.join("bin/rust-analyzer")).unwrap();
    let environment = EnvironmentFixture {
        home: root.clone(),
        shell: "/bin/false".into(),
        environment: BTreeMap::from([("PATH".into(), root.join("bin").into_os_string())]),
        timeout: Duration::from_secs(1),
    };
    fixture.node = fixture
        .runtime
        .block_on(Node::start_with_code(
            root.join("node"),
            environment,
            &network.endpoint,
        ))
        .unwrap();
    let address = fixture
        .runtime
        .block_on(
            fixture
                .controller
                .link()
                .pair(fixture.node.link().invite().unwrap().ticket()),
        )
        .unwrap();
    let transport = if remote {
        fixture.controller.link().remote(address)
    } else {
        fixture.node.local()
    };
    let observed = Arc::new(observed::Observed::new(transport));
    let client = Arc::new(sailry_client::Client::new(observed.clone()));
    fixture.binding.client = client.clone();
    fixture.binding.defaults = client;
    (fixture, observed)
}

fn discovery(fixture: &Fixture) -> code::Result {
    let Output::Code(result) = fixture.execute(Command::ReadCode {
        worktree: fixture.session.worktree,
        query: code::Query::Diagnostics {
            path: "main.rs".into(),
            severity: None,
        },
        overlay: None,
    }) else {
        panic!("code result expected")
    };
    result
}

fn marker_empty(input: &Entity<EditorState>, cx: &App) -> bool {
    input
        .read(cx)
        .diagnostics()
        .is_none_or(|items| items.is_empty())
}

#[gpui_kit::test]
fn discovers_installs_and_restarts_removed_servers(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let network = network::Network::new();
        let (fixture, observed) = managed(remote, &network);
        observed.fail_subscription.store(true, Ordering::SeqCst);
        let (panel, visual) = publication::mount(&fixture, cx);
        let (controller, id, input) = open(&panel, visual, "resource-file-main.rs");
        let missing = discovery(&fixture);
        assert_eq!(missing.status, code::Status::NotInstalled);
        assert!(missing.startup.is_none());
        wait(visual, |cx| marker_empty(&input, cx));
        let install = missing.package_info.unwrap().install.unwrap();
        assert_eq!(install.version, network.asset.version);
        let bin = fixture
            .node
            .profile()
            .join("tools/lsp")
            .join(&install.package)
            .join(&install.version)
            .join("bin");
        let state = fixture.directory.path().join("project");
        assert!(!state.join("pid").exists());
        network.held.store(true, Ordering::SeqCst);
        let client = fixture.binding.client.clone();
        let request = client.prepare(Command::InstallCodeServer {
            install: install.clone(),
        });
        let installing = fixture
            .runtime
            .spawn(async move { client.execute(request).await });
        wait(visual, |_| network.hits.load(Ordering::SeqCst) == 1);
        assert!(controller.read_with(visual, |controller, _| {
            controller.documents[&id]
                .code
                .as_ref()
                .unwrap()
                .startup
                .borrow()
                .is_none()
        }));
        network.held.store(false, Ordering::SeqCst);
        let Output::CodeInstallation(installed) =
            fixture.runtime.block_on(installing).unwrap().unwrap()
        else {
            panic!("installation expected")
        };
        assert_eq!(installed.status, code::InstallStatus::Ready);
        cold(&controller, id, visual);
        assert!(!state.join("pid").exists());
        start(visual);
        ready(&input, visual);
        assert!(observed.subscriptions.load(Ordering::SeqCst) >= 2);
        assert!(!observed.fail_subscription.load(Ordering::SeqCst));
        let startup = discovery(&fixture).server.unwrap();
        assert_eq!(startup, "rust-analyzer");
        let original = std::fs::read_to_string(state.join("pid")).unwrap();
        fixture.execute(Command::UninstallCodeServer {
            package: install.package.clone(),
            version: install.version.clone(),
        });
        wait(visual, |cx| {
            marker_empty(&input, cx)
                && controller.read(cx).documents[&id]
                    .code
                    .as_ref()
                    .is_some_and(|provider| provider.startup.borrow().is_none())
        });
        assert!(!bin.exists());
        let client = &fixture.binding.client;
        let reinstalled = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::InstallCodeServer { install })))
            .unwrap_or_else(|error| {
                panic!(
                    "second installation failed (remote={remote}): {error}; {}",
                    network.diagnostics()
                )
            });
        let Output::CodeInstallation(reinstalled) = reinstalled else {
            panic!("installation expected")
        };
        assert_eq!(reinstalled.status, code::InstallStatus::Ready);
        ready(&input, visual);
        assert_eq!(network.hits.load(Ordering::SeqCst), 2);
        let restarted = std::fs::read_to_string(state.join("pid")).unwrap();
        assert_ne!(original, restarted);
        assert_eq!(
            input.read_with(visual, |input, _| input.value().to_string()),
            "🙂 broken symbol\n"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/main.rs")).unwrap(),
            "🙂 broken symbol\n"
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(input);
        fixture.close();
    }
}

#[gpui_kit::test]
fn restarts_after_process_failure(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote, Permission::Ask);
        let (panel, visual) = publication::mount(&fixture, cx);
        let (controller, id, input) = open(&panel, visual, "resource-file-main.rs");
        cold(&controller, id, visual);
        start(visual);
        ready(&input, visual);
        let pid = std::fs::read_to_string(fixture.directory.path().join("project/pid")).unwrap();
        let killed = std::process::Command::new("kill")
            .args(["-TERM", pid.trim()])
            .status()
            .unwrap();
        assert!(killed.success());
        wait(visual, |_| calls(&fixture, "initialize") == 2);
        ready(&input, visual);
        assert_ne!(
            std::fs::read_to_string(fixture.directory.path().join("project/pid")).unwrap(),
            pid
        );
        assert_eq!(
            input.read_with(visual, |input, _| input.value().to_string()),
            "🙂 broken symbol\n"
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(input);
        fixture.close();
    }
}

#[gpui_kit::test]
fn clears_confirmation_for_dismissed_documents(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = fixture(remote, Permission::Ask);
        let publication = Arc::new(super::super::observed::Observed::new(
            fixture.transport.clone(),
        ));
        let _publication_release = publication.release_on_drop();
        let observed = Arc::new(observed::Observed::new(publication.clone()));
        fixture.binding.client = Arc::new(sailry_client::Client::new(observed.clone()));
        let (panel, visual) = publication::mount(&fixture, cx);
        let (controller, id, input) = open(&panel, visual, "resource-file-main.rs");
        cold(&controller, id, visual);
        publication::replace(&panel, visual, &"An uncertain broken draft\n".repeat(9_000));
        publication.mode.store(3, Ordering::SeqCst);
        click(visual, "document-save");
        wait(visual, |cx| controller.read(cx).documents[&id].uncertain);
        let release = observed.hold_snapshot();
        click(visual, "editor-code-start");
        wait(visual, |cx| {
            observed.entered.is_cancelled() && controller.read(cx).confirming_code(id)
        });
        click(visual, "document-discard");
        wait(visual, |cx| controller.read(cx).documents[&id].dismissed);
        drop(release);
        wait(visual, |cx| !controller.read(cx).confirming_code(id));
        let (_, reopened, retained) = open(&panel, visual, "resource-file-main.rs");
        assert_eq!(reopened, id);
        assert_eq!(retained.entity_id(), input.entity_id());
        click(visual, "editor-code-start");
        consent(visual);
        assert!(!fixture.directory.path().join("project/pid").exists());
        visual.update(|window, cx| {
            window.close_dialog(cx);
            window.remove_window();
        });
        drop(panel);
        drop(controller);
        drop(input);
        drop(retained);
        fixture.close();
    }
}
