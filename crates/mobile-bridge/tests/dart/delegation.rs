use super::{configuration, configuration::support, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{conversation::Status, *};
use serde_json::json;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn recovers_and_stops_children() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("source.txt"), "Read 子任务 🙂").unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let text = "完整子任务结果 🙂\n".repeat(200);
    let parent = runtime.block_on(support::Server::parallel(vec![
        (
            super::agent_support::plugin_tool("delegation", "spawn_agent"),
            json!({"role": "review", "task": "独立任务 A 🙂"}),
        ),
        (
            super::agent_support::plugin_tool("delegation", "spawn_agent"),
            json!({"role": "review", "task": "独立任务 B 🙂"}),
        ),
    ]));
    let child = runtime.block_on(support::Server::tools(vec![
        (
            super::agent_support::plugin_tool("files", "read_file"),
            json!({"path": "source.txt"}),
        ),
        (
            super::agent_support::plugin_tool("files", "write_file"),
            json!({"path": "child.txt", "text": text, "expected_revision": null}),
        ),
    ]));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &parent.endpoint,
        "ffi-parent",
    ));
    runtime.block_on(async {
        let client = Client::new(node.local());
        let mut provider = configuration::provider(&client, &child.endpoint, "ffi-child").await;
        provider.models[0].tools = true;
        client
            .execute(client.prepare(Command::PutProvider {
                provider: provider.clone(),
                expected_revision: provider.revision,
            }))
            .await
            .unwrap();
        let role = role::Profile {
            appearance: None,
            id: RoleId::new(),
            revision: 0,
            key: "review".into(),
            name: "Review 中文".into(),
            description: "Inspect the assigned task".into(),
            model: Some(role::Model {
                provider: provider.id,
                model: "ffi-child".into(),
                effort: None,
            }),
            max_turns: Some(5),
            skills: vec![],
            instructions: "Frozen child instructions\n完整角色 🙂".into(),
        };
        let Output::Role(role) = client
            .execute(client.prepare(Command::PutRole {
                role,
                expected_revision: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("role expected")
        };
        for session in &sessions {
            client
                .execute(client.prepare(Command::SetSessionRoles {
                    session: *session,
                    expected_revision: 1,
                    roles: vec![role.reference()],
                }))
                .await
                .unwrap();
        }
    });
    let invitation = node.link().invite().unwrap();
    let process = super::command("delegation.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSIONS", serde_json::to_string(&sessions).unwrap())
        .spawn()
        .unwrap();
    let status = super::wait(process);
    let (snapshot, pages) = runtime.block_on(async {
        let client = Client::new(node.local());
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let ids = snapshot
            .sessions
            .iter()
            .map(|session| session.id)
            .collect::<Vec<_>>();
        let pages = tools::pages(&client, &ids).await;
        node.shutdown().await.unwrap();
        (snapshot, pages)
    });
    assert!(status.success());
    assert_eq!(snapshot.sessions.len(), 6);
    assert_eq!(snapshot.turns.len(), 6);
    assert_eq!(
        snapshot
            .sessions
            .iter()
            .filter(|session| session.delegation.is_some())
            .count(),
        4
    );
    assert_eq!(
        pages
            .iter()
            .filter(|page| page.runs[0].status == Status::Completed)
            .count(),
        3
    );
    assert_eq!(
        pages
            .iter()
            .filter(|page| page.runs[0].status == Status::Cancelled)
            .count(),
        3
    );
    assert_eq!(
        std::fs::read_to_string(root.join("child.txt")).unwrap(),
        text
    );
    runtime.block_on(async {
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(node.local());
        let ids = snapshot
            .sessions
            .iter()
            .map(|session| session.id)
            .collect::<Vec<_>>();
        assert_eq!(tools::pages(&client, &ids).await, pages);
        let Output::Snapshot(restored) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert_eq!(restored.sessions, snapshot.sessions);
        assert_eq!(restored.turns, snapshot.turns);
        node.shutdown().await.unwrap();
    });
    assert_eq!(parent.requests.lock().unwrap().len(), 3);
    let requests = child.requests.lock().unwrap();
    assert_eq!(requests.len(), 10);
    for request in requests.iter() {
        assert_eq!(request["model"], "ffi-child");
        let messages = request["messages"].to_string();
        assert!(messages.contains("Frozen child instructions"));
        assert!(messages.contains("完整角色 🙂"));
        assert!(!messages.contains("Parent-only context"));
        assert_ne!(
            messages.contains("独立任务 A 🙂"),
            messages.contains("独立任务 B 🙂")
        );
    }
    assert!(child.authorization.lock().unwrap().iter().all(|header| header.as_deref() == Some("Bearer isolated-ffi-configuration-credential")));
}
