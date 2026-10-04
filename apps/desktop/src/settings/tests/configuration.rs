use super::*;
use crate::{backend::Services, preview::Page, shell::Shell};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Output};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[track_caller]
fn wait(cx: &mut VisualTestContext, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.update(|window, cx| {
            let _ = window.draw(cx);
            predicate(cx)
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "configuration routing deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn keeps_execution_selection(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let local = runtime
        .block_on(Node::start(directory.path().join("local")))
        .unwrap();
    let remote = runtime
        .block_on(Node::start(directory.path().join("remote")))
        .unwrap();
    runtime
        .block_on(local.link().pair(remote.link().invite().unwrap().ticket()))
        .unwrap();
    let client = Client::new(remote.local());
    let Output::Project(project) = runtime
        .block_on(client.execute(client.prepare(Command::RegisterProject {
            name: "Settings routing".into(),
            path: directory.path().to_str().unwrap().into(),
        })))
        .unwrap()
    else {
        panic!("project expected")
    };
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(Services {
            runtime: runtime.clone(),
            local: local.local(),
            link: local.link(),
            relay_enabled: false,
        });
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
    wait(visual, |cx| {
        shell
            .read(cx)
            .live
            .as_ref()
            .unwrap()
            .hosts
            .contains_key(&remote.id())
    });
    visual.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.live.as_mut().unwrap().select(remote.id(), cx)
        })
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
            .is_some_and(|snapshot| !snapshot.projects.is_empty())
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.live.as_mut().unwrap().project = Some(project.id);
            shell.new_live_conversation(window, cx);
        })
    });
    wait(visual, |cx| {
        shell
            .read(cx)
            .current_chat()
            .is_some_and(|view| view.read(cx).connected())
    });
    let draft = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().entity_id());
    assert!(visual.debug_bounds("live-chat-configure").is_none());
    let bounds = visual.debug_bounds("live-chat-model").unwrap();
    visual.simulate_click(bounds.center(), Modifiers::default());
    wait(visual, |cx| {
        shell
            .read(cx)
            .settings
            .read(cx)
            .provider_link
            .as_ref()
            .is_some_and(|link| link.connected && link.binding.client.target() == remote.id())
    });
    // Explicit configuration-owner navigation must not switch the execution target.
    visual.update(|_, cx| {
        let chat = shell.read(cx).current_chat().unwrap().clone();
        chat.update(cx, |_, cx| {
            cx.emit(crate::conversation::live::Event::Settings(local.id()))
        });
    });
    wait(visual, |cx| {
        shell
            .read(cx)
            .settings
            .read(cx)
            .provider_link
            .as_ref()
            .is_some_and(|link| link.connected && link.binding.client.target() == local.id())
    });
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.page),
        Page::Settings
    );
    assert_eq!(
        shell.read_with(visual, |shell, cx| shell.settings.read(cx).section),
        Section::Providers
    );
    assert!(visual.debug_bounds("composer-model-picker").is_none());
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.live.as_ref().unwrap().selected),
        remote.id()
    );
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.navigate(Page::Conversation, window, cx)
        })
    });
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.current_chat().unwrap().entity_id()),
        draft
    );
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.live.as_ref().unwrap().project),
        Some(project.id)
    );
    // Ordinary settings navigation still follows the selected execution Node.
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.navigate(Page::Settings, window, cx))
    });
    wait(visual, |cx| {
        shell
            .read(cx)
            .settings
            .read(cx)
            .provider_link
            .as_ref()
            .is_some_and(|link| link.connected && link.binding.client.target() == remote.id())
    });
    visual.update(|window, _| window.remove_window());
    runtime.block_on(local.shutdown()).unwrap();
    runtime.block_on(remote.shutdown()).unwrap();
}
