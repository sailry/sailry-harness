use super::*;
use sailry_protocol::{Output, conversation::Status};

mod assistants;
mod goals;

fn package(fixture: &Fixture, control: bool) -> sailry_protocol::plugin::Info {
    let root = fixture.directory.path().join("project/package");
    for (path, content) in [
        (
            "plugin.json",
            include_str!("../../../../../plugins/examples/task-notes/plugin.json"),
        ),
        (
            "dev.sailry.platform/desktop/main.js",
            include_str!(
                "../../../../../plugins/examples/task-notes/dev.sailry.platform/desktop/main.js"
            ),
        ),
        (
            "dev.sailry.platform/desktop/locales.js",
            include_str!(
                "../../../../../plugins/examples/task-notes/dev.sailry.platform/desktop/locales.js"
            ),
        ),
        (
            "dev.sailry.platform/desktop/chat.js",
            include_str!(
                "../../../../../plugins/examples/task-notes/dev.sailry.platform/desktop/chat.js"
            ),
        ),
    ] {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    if !control {
        let path = root.join("plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["actions"]
            .as_array_mut()
            .unwrap()
            .retain(|action| action != "conversation.control");
        std::fs::write(path, manifest.to_string()).unwrap();
    }
    let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "package".into(),
        name: "task-notes".into(),
        expected_revision: 0,
    }) else {
        panic!("plugin expected")
    };
    info
}

#[gpui::test]
fn embeds_shared_history_and_releases_without_stopping(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = package(&fixture, true);
        let started = std::sync::Arc::new(tokio::sync::Notify::new());
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let server = fixture.runtime.block_on(crate::agent_fixture::Server::held(
            started.clone(),
            release.clone(),
        ));
        let Output::Snapshot(state) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let mut provider = state.providers[0].clone();
        provider.endpoint = server.endpoint.clone();
        fixture.execute(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        });
        let (panel, visual) = mount(&fixture, cx);
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.open(package.summary.reference(), window, cx)
            })
        });
        wait(visual, |cx| {
            panel
                .read(cx)
                .mounted
                .as_ref()
                .and_then(|mounted| mounted.conversations.first())
                .is_some_and(|chat| chat.read(cx).connected())
        });
        let chat = panel.read_with(visual, |panel, _| {
            panel
                .mounted
                .as_ref()
                .unwrap()
                .conversations
                .first()
                .unwrap()
        });
        chat.read_with(visual, |chat, _| {
            assert_eq!(chat.session(), Some(fixture.session.id));
            assert_eq!(chat.binding().client.target(), fixture.node.id());
        });
        // The original view and embedded view share Node state, not draft/focus entities.
        assert_ne!(
            chat.entity_id(),
            panel.read_with(visual, |panel, _| panel
                .source
                .as_ref()
                .unwrap()
                .entity_id())
        );
        click(visual, "live-chat-input");
        visual.simulate_input("Retain this task after the panel closes");
        visual.simulate_keystrokes("enter");
        wait(visual, |_| {
            let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
                session: fixture.session.id,
                before: None,
                limit: 100,
            }) else {
                return false;
            };
            history
                .page
                .runs
                .iter()
                .any(|run| run.status == Status::Running)
                && !server.requests.lock().unwrap().is_empty()
        });
        let weak = chat.downgrade();
        drop(chat);
        fixture.execute(Command::SetPluginEnabled {
            name: package.summary.name.clone(),
            enabled: false,
            expected_revision: package.summary.revision,
        });
        wait(visual, |cx| {
            panel.read(cx).mounted.is_none() && weak.upgrade().is_none()
        });
        let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
            session: fixture.session.id,
            before: None,
            limit: 100,
        }) else {
            panic!("history expected")
        };
        assert!(
            history
                .page
                .runs
                .iter()
                .any(|run| run.status == Status::Running)
        );
        started.notify_one();
        release.notify_one();
        wait(visual, |_| {
            let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
                session: fixture.session.id,
                before: None,
                limit: 100,
            }) else {
                return false;
            };
            history
                .page
                .runs
                .iter()
                .any(|run| run.status == Status::Completed)
        });
        assert!(!server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn requires_declared_conversation_control(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::new(true);
    let package = package(&fixture, false);
    let (panel, visual) = mount(&fixture, cx);
    visual.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open(package.summary.reference(), window, cx)
        })
    });
    wait(visual, |cx| panel.read(cx).mounted.is_some());
    assert!(
        visual
            .debug_bounds("plugin-conversation-unavailable")
            .is_some()
    );
    assert!(panel.read_with(visual, |panel, _| {
        panel
            .mounted
            .as_ref()
            .unwrap()
            .conversations
            .first()
            .is_none()
    }));
    visual.update(|window, _| window.remove_window());
    drop(panel);
    fixture.close();
}
