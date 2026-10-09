use super::*;
use crate::settings::{Section, Workspace};
use core::prelude::v1::test;
use sailry_client::Client;
use sailry_node_runtime::Node;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn wait(cx: &mut VisualTestContext, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            ready(cx)
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "sandbox settings UI deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn read(runtime: &tokio::runtime::Runtime, client: &Client) -> Settings {
    let Output::SandboxSettings(settings) = runtime
        .block_on(client.execute(client.prepare(Command::ReadSandboxSettings)))
        .unwrap()
    else {
        panic!("sandbox settings expected")
    };
    settings
}

#[gpui::test]
fn saves_the_selected_host_and_observes_changes(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let local = runtime
        .block_on(Node::start_isolated(directory.path().join("local")))
        .unwrap();
    let remote = runtime
        .block_on(Node::start_isolated(directory.path().join("remote")))
        .unwrap();
    let address = runtime
        .block_on(local.link().pair(remote.link().invite().unwrap().ticket()))
        .unwrap();
    let transports: [Arc<dyn sailry_link::Transport>; 2] =
        [local.local(), local.link().remote(address)];
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        cx.set_global(crate::preferences::Preferences::open(
            directory.path().join("preferences.json"),
        ));
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        workspace.update(cx, |workspace, cx| {
            workspace.select(Section::Conversation, cx)
        });
        owner = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    let owner = owner.unwrap();
    for (index, transport) in transports.iter().enumerate() {
        let node = transport.target();
        visual.update(|_, cx| {
            owner.update(cx, |workspace, cx| {
                workspace.bind_providers(
                    transport.clone(),
                    runtime.clone(),
                    format!("Host {index}").into(),
                    cx,
                )
            })
        });
        wait(visual, |cx| {
            owner
                .read(cx)
                .conversation
                .hosts
                .get(&node)
                .is_some_and(|host| host.read(cx).saved.is_some())
        });
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(
            handle,
            size(px(if index == 0 { 420. } else { 960. }), px(820.)),
        );
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let switch = visual.debug_bounds("command-sandbox").unwrap();
        assert!(
            switch.left() >= px(0.) && switch.right() <= px(if index == 0 { 420. } else { 960. })
        );
        visual.simulate_click(switch.center(), Modifiers::default());
        wait(visual, |cx| {
            owner.read(cx).conversation.hosts[&node]
                .read(cx)
                .saved
                .as_ref()
                .is_some_and(|settings| !settings.enabled && settings.revision == 1)
        });
        let client = Client::new(transport.clone());
        assert_eq!(
            read(&runtime, &client),
            Settings {
                revision: 1,
                enabled: false
            }
        );
        if index == 0 {
            assert_eq!(
                read(&runtime, &Client::new(transports[1].clone())),
                Settings::default()
            );
        }
        // Another controller's change must be reduced and rendered by the same setting.
        let mut changed = read(&runtime, &client);
        changed.enabled = true;
        runtime
            .block_on(client.execute(client.prepare(Command::SaveSandboxSettings(changed))))
            .unwrap();
        wait(visual, |cx| {
            owner.read(cx).conversation.hosts[&node]
                .read(cx)
                .saved
                .as_ref()
                .is_some_and(|settings| settings.enabled && settings.revision == 2)
        });
        visual.update(|_, cx| {
            owner.update(cx, |workspace, cx| {
                workspace.select(Section::General, cx);
                workspace.select(Section::Conversation, cx);
            })
        });
        wait(visual, |cx| {
            owner.read(cx).conversation.hosts[&node]
                .read(cx)
                .saved
                .as_ref()
                .unwrap()
                .enabled
        });
    }
    visual.update(|window, _| window.remove_window());
    runtime.block_on(local.shutdown()).unwrap();
    runtime.block_on(remote.shutdown()).unwrap();
}

#[gpui::test]
fn preview_does_not_open_a_node(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        workspace.update(cx, |workspace, cx| {
            workspace.select(Section::Conversation, cx)
        });
        owner = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let switch = visual.debug_bounds("command-sandbox").unwrap();
    visual.simulate_click(switch.center(), Modifiers::default());
    visual.run_until_parked();
    let owner = owner.unwrap();
    owner.read_with(visual, |workspace, cx| {
        assert!(workspace.provider_link.is_none());
        assert!(workspace.conversation.hosts.is_empty());
        assert!(
            !workspace
                .conversation
                .preview
                .read(cx)
                .saved
                .as_ref()
                .unwrap()
                .enabled
        );
    });
}
