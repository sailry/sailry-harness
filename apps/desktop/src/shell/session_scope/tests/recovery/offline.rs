use super::*;
use crate::panes::Target;
use gpui_kit::{
    base::Placement,
    component::dock::{DockPlacement, DropTarget, PanelId},
};
use sailry_node_runtime::{NetworkScope, Node};
use sailry_protocol::{
    Effort, Permission, ProviderId, SessionConfig, WorkMode,
    terminal::{Launch, Status as TerminalStatus, Viewport},
};

fn wait_terminal(cx: &mut VisualTestContext) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        if cx.debug_bounds("terminal-grid").is_some()
            && cx.debug_bounds("terminal-status").is_none()
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "restored terminal connection deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn retains_remote_restart_layout(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let local = runtime
        .block_on(Node::start(directory.path().join("controller")))
        .unwrap();
    let profile = directory.path().join("remote");
    let remote = runtime.block_on(Node::start(&profile)).unwrap();
    let node = remote.id();
    let address = runtime
        .block_on(local.link().pair(remote.link().invite().unwrap().ticket()))
        .unwrap();
    let socket = *address.ip_addrs().next().unwrap();
    let client = Client::new(local.link().remote(address));
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let execute = |command| {
        runtime
            .block_on(client.execute(client.prepare(command)))
            .unwrap()
    };
    let Output::Project(project) = execute(Command::RegisterProject {
        name: "Offline split fixture".into(),
        path: root.to_str().unwrap().into(),
    }) else {
        panic!("project expected")
    };
    let Output::Session(session) = execute(Command::CreateSession {
        project: Some(project.id),
        worktree: None,
        config: Some(SessionConfig {
            assistant: None,
            resource: None,
            provider: ProviderId::new(),
            model: "fixture".into(),
            effort: Effort::Default,
            mode: WorkMode::Code,
            permission: Permission::Ask,
            credential: None,
        }),
    }) else {
        panic!("session expected")
    };
    let services = crate::backend::Services {
        runtime: runtime.clone(),
        link: local.link(),
        local: local.local(),
        relay_enabled: false,
    };
    let (shell, visual) = mount_with_services(cx, services);
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1400.), px(900.)));
    let Output::Terminal(info) = execute(Command::CreateTerminal(Launch {
        worktree: session.worktree,
        viewport: Viewport {
            columns: 80,
            rows: 24,
            pixel_width: 0,
            pixel_height: 0,
        },
        appearance: visual.update(|_, cx| crate::theme::terminal(cx)),
    })) else {
        panic!("terminal expected")
    };
    visual.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.live.as_mut().unwrap().select(node, cx)
        });
    });
    wait(visual, |cx| {
        shell
            .read(cx)
            .live
            .as_ref()
            .unwrap()
            .view
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.node == node)
    });
    let conversation = Target::Session(node, session.id);
    let terminal = Target::Terminal(node, session.worktree, info.id);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.reveal_session(session.clone(), window, cx);
            shell.reveal_terminal(Some(project.id), &info, window, cx);
            shell.splits.update(cx, |splits, cx| {
                splits.focus(conversation, cx);
                let workspace = splits.workspace(conversation, cx).unwrap();
                let panel = PanelId::from(splits.pane(conversation).unwrap().entity_id());
                let area = splits.active_area(cx).unwrap();
                let target = area
                    .read(cx)
                    .layout(DockPlacement::Center)
                    .unwrap()
                    .find_panel_node(panel)
                    .unwrap();
                assert!(splits.drop(
                    workspace,
                    terminal,
                    DropTarget::new(target, Some(Placement::Right)),
                    window,
                    cx,
                ));
            });
        });
    });
    wait(visual, |cx| {
        shell.read(cx).chats.views[&(node, session.id)]
            .read(cx)
            .connected()
    });
    wait_terminal(visual);
    let saved = shell.read_with(visual, |shell, cx| shell.splits.read(cx).saved(cx));
    assert_eq!(saved.active, Some(terminal));
    assert_eq!(saved.parents, [(terminal, conversation)]);
    assert_eq!(saved.groups.len(), 1);
    visual.update(|window, _| window.remove_window());
    drop(shell);
    runtime.block_on(remote.shutdown()).unwrap();

    // Recreate the desktop shell while its paired execution Node is stopped.
    cx.update(|cx| {
        crate::preferences::update(cx, |preferences| {
            preferences.workspaces = Some(serde_json::to_value(&saved).unwrap());
        });
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1400.), px(900.)));
    let panes = shell.read_with(visual, |shell, cx| {
        [conversation, terminal]
            .map(|target| shell.splits.read(cx).pane(target).unwrap().entity_id())
    });
    let retries = [conversation, terminal].map(|target| -> &'static str {
        Box::leak(format!("pane-restore-retry-{target:?}").into_boxed_str())
    });
    // Link serializes connection attempts and each real offline dial has a timeout.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if retries
            .iter()
            .all(|selector| visual.debug_bounds(selector).is_some())
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "offline split placeholders deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let layout = shell.read_with(visual, |shell, cx| {
        let splits = shell.splits.read(cx);
        assert_eq!(shell.page, Page::Terminal);
        assert_eq!(splits.active, Some(terminal));
        assert!(!splits.contains(Target::Draft));
        assert!(shell.chats.views.is_empty());
        assert_eq!(splits.parent(terminal), Some(conversation));
        assert_eq!(
            splits.workspace(conversation, cx),
            splits.workspace(terminal, cx)
        );
        serde_json::to_value(splits.saved(cx)).unwrap()
    });
    assert!(visual.debug_bounds("terminal-component").is_none());

    let remote = runtime
        .block_on(Node::start_with_network(
            &profile,
            NetworkScope::Direct(socket),
        ))
        .unwrap();
    assert_eq!(remote.id(), node);
    let Output::Snapshot(snapshot) = execute(Command::Snapshot) else {
        panic!("snapshot expected")
    };
    assert!(snapshot.sessions.iter().any(|entry| entry.id == session.id));
    assert_eq!(
        snapshot
            .terminals
            .iter()
            .find(|entry| entry.id == info.id)
            .unwrap()
            .status,
        TerminalStatus::Stopped,
    );
    for retry in &retries {
        click(&shell, visual, retry.to_string());
    }
    wait(visual, |cx| {
        let shell = shell.read(cx);
        shell
            .chats
            .views
            .get(&(node, session.id))
            .is_some_and(|view| view.read(cx).connected())
    });
    wait_terminal(visual);
    let Output::TerminalSnapshot(snapshot) =
        execute(Command::InspectTerminal { terminal: info.id })
    else {
        panic!("terminal snapshot expected")
    };
    assert_eq!(snapshot.info.status, TerminalStatus::Running);
    assert!(snapshot.info.revision > info.revision);
    assert_eq!(snapshot.info.owner, Some(local.id()));
    shell.read_with(visual, |shell, cx| {
        let splits = shell.splits.read(cx);
        assert_eq!(shell.page, Page::Terminal);
        assert_eq!(splits.active, Some(terminal));
        assert_eq!(serde_json::to_value(splits.saved(cx)).unwrap(), layout);
        assert_eq!(
            [conversation, terminal].map(|target| splits.pane(target).unwrap().entity_id()),
            panes,
        );
        let live = shell.live.as_ref().unwrap();
        assert_eq!(live.selected, node);
        assert_eq!(live.project, Some(project.id));
        assert_eq!(live.selected_worktree().unwrap().id, session.worktree);
    });
    for retry in retries {
        assert!(visual.debug_bounds(retry).is_none());
    }
    visual.update(|window, _| window.remove_window());
    drop(shell);
    runtime.block_on(remote.shutdown()).unwrap();
    runtime.block_on(local.shutdown()).unwrap();
}
