use super::*;
use sailry_protocol::conversation::{ModelApi, Provider, presets::*};
use serde_json::json;
#[path = "providers/adapter.rs"]
mod adapter;

fn declare(root: &Path, value: u32) {
    package(root, &format!("1.0.{value}"));
    let path = root.join("plugin.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["extensions"] = json!({"dev.sailry.platform":{"api_version":"v1","model_providers":[{
        "id":"example","name":"Example channel","routes":[{"protocol":"openai-chat-completions","endpoint":"http://localhost:11434/v1"}],
        "policies":[{"models":["fixture"],"body":{"example":value}}]
    }]}});
    fs::write(path, manifest.to_string()).unwrap();
}

fn captured(profile: &Path, turn: TurnId) -> Provider {
    let db = rusqlite::Connection::open_with_flags(
        profile.join("storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let body: Vec<u8> = db
        .query_row(
            "SELECT provider FROM agent_runs WHERE turn=?1",
            [turn.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn freezes_updates_at_turn_admission() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let source = directory.path().join("source/package");
        declare(&source, 1);
        let server = model_fixture::Server::start(false).await;
        let first = info(execute(&client, install(worktree, 0)).await);
        assert!(first.issues.is_empty(), "{:?}", first.issues);
        let Output::ChannelPresets(listing) = execute(
            &client,
            Command::ListChannelPresets {
                refresh_partners: false,
            },
        )
        .await
        else {
            panic!("channel catalog expected")
        };
        let entry = listing
            .entries
            .iter()
            .find(|entry| matches!(&entry.source,Source::Package{name} if name=="example"))
            .unwrap();
        assert_eq!(entry.package, Some(first.summary.reference()));
        let reference = PresetRef {
            source: entry.source.clone(),
            id: entry.preset.id.clone(),
            route: Some(entry.preset.routes[0].key()),
        };
        let mut provider:Provider=serde_json::from_value(json!({"id":ProviderId::new(),"revision":0,"name":"Example","api":"chat_completions","authentication":"api_key","endpoint":"http://localhost:11434/v1","enabled":true,"models":[],"default_model":"","credential":null,"preset":reference})).unwrap();
        provider.endpoint = server.endpoint.clone();
        let Output::Provider(provider) = execute(
            &client,
            Command::PutProvider {
                provider,
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        assert!(provider.binding.is_none());
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let project = snapshot
            .worktrees
            .iter()
            .find(|item| item.id == worktree)
            .unwrap()
            .project;
        let Output::Session(session) = execute(
            &client,
            Command::CreateSession {
                project,
                worktree: Some(worktree),
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: provider.id,
                    credential: None,
                    model: "fixture".into(),
                    effort: Effort::Default,
                    mode: WorkMode::Code,
                    permission: Permission::Ask,
                    options: Default::default(),
                }),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        let queue = || Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Capture the declaration".into(),
        };
        let Output::QueuedTurn(first_turn) = execute(&client, queue()).await else {
            panic!("turn expected")
        };
        finish(&client, session.id, first_turn.id).await;
        let original = captured(node.profile(), first_turn.id);
        assert_eq!(
            original.binding.as_ref().unwrap().package,
            Some(first.summary.reference())
        );
        declare(&source, 2);
        let second = info(execute(&client, install(worktree, first.summary.revision)).await);
        let Output::QueuedTurn(second_turn) = execute(&client, queue()).await else {
            panic!("turn expected")
        };
        finish(&client, session.id, second_turn.id).await;
        let current = captured(node.profile(), second_turn.id);
        assert_eq!(
            current.binding.as_ref().unwrap().package,
            Some(second.summary.reference())
        );
        assert_eq!(
            current
                .binding
                .as_ref()
                .unwrap()
                .policies
                .last()
                .unwrap()
                .body["example"],
            2
        );
        assert_eq!(captured(node.profile(), first_turn.id), original);
        assert_eq!(server.requests.lock().unwrap()[0]["example"], 1);
        assert_eq!(server.requests.lock().unwrap()[1]["example"], 2);
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "example".into(),
                expected_revision: second.summary.revision,
                enabled: false,
            },
        )
        .await;
        assert_eq!(
            client
                .execute(client.prepare(queue()))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        assert_eq!(captured(node.profile(), first_turn.id), original);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

async fn finish(client: &Client, session: SessionId, turn: TurnId) {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let page = client
                .read_conversation(session, None, 20)
                .await
                .unwrap()
                .page;
            if let Some(run) = page.runs.iter().find(|run| run.turn == turn)
                && matches!(
                    run.status,
                    sailry_protocol::conversation::Status::Completed
                        | sailry_protocol::conversation::Status::Failed
                )
            {
                assert_eq!(
                    run.status,
                    sailry_protocol::conversation::Status::Completed,
                    "{:?}",
                    run.error
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
