use super::*;
use sailry_protocol::conversation::{Model, ModelApi, Provider};

pub(super) async fn session(client: &Client, worktree: WorktreeId) -> Session {
    let provider = Provider {
        options: None,
        id: ProviderId::new(),
        revision: 0,
        name: "Action scope fixture".into(),
        api: ModelApi::ChatCompletions,
        authentication: sailry_protocol::Authentication::ApiKey,
        endpoint: "http://127.0.0.1:9/v1".into(),
        enabled: true,
        default_model: "fixture".into(),
        credential: None,
        models: vec![Model {
            id: "fixture".into(),
            context: 128,
            output: 32,
            vision: false,
            tools: true,
            reasoning: false,
            web_search: false,
            generates: vec![],
            efforts: vec![],
            custom_efforts: false,
            default_effort: sailry_protocol::Effort::Default,
        }],
    };
    execute(
        client,
        Command::PutProvider {
            provider: provider.clone(),
            expected_revision: 0,
        },
    )
    .await;
    let Output::Snapshot(snapshot) = execute(client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    let project = snapshot
        .worktrees
        .iter()
        .find(|entry| entry.id == worktree)
        .unwrap()
        .project;
    let Output::Session(session) = execute(
        client,
        Command::CreateSession {
            project,
            worktree: Some(worktree),
            config: Some(SessionConfig {
                assistant: None,
                resource: None,
                provider: provider.id,
                model: "fixture".into(),
                effort: Effort::Low,
                mode: sailry_protocol::WorkMode::Code,
                permission: Permission::Ask,
                credential: None,
            }),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    session
}

#[tokio::test]
async fn reads_captured_session() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source");
        let mut context = install_actions(
            &client,
            &root.join("package"),
            worktree,
            0,
            &[Action::ReadConversation, Action::WriteFiles],
        )
        .await;
        let first = session(&client, worktree).await;
        let second = session(&client, worktree).await;
        context.session = Some(first.id);
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::ReadSession { session: first.id })
                        .with_plugin(context.clone())
                )
                .await
                .unwrap(),
            Output::Session(first.clone())
        );
        let denied = client
            .prepare(Command::SetSessionConfig {
                session: first.id,
                expected_revision: first.revision,
                config: first.config.clone(),
            })
            .with_plugin(context.clone());
        assert_eq!(
            client.execute(denied).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        let command = Command::ReadConversation {
            session: first.id,
            before: None,
            limit: 10,
        };
        let expected = execute(&client, command.clone()).await;
        assert_eq!(
            client
                .execute(client.prepare(command).with_plugin(context.clone()))
                .await
                .unwrap(),
            expected
        );
        for selected in [Some(first.id), None] {
            context.session = selected;
            let request = client
                .prepare(Command::ReadConversation {
                    session: second.id,
                    before: None,
                    limit: 10,
                })
                .with_plugin(context.clone());
            assert_eq!(
                client.execute(request).await.unwrap_err().code,
                ErrorCode::PermissionDenied
            );
        }
        let other = directory.path().join("other");
        fs::create_dir(&other).unwrap();
        let Output::Project(project) = execute(
            &client,
            Command::RegisterProject {
                name: "Other scope".into(),
                path: other.to_str().unwrap().into(),
            },
        )
        .await
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|entry| entry.project == Some(project.id))
            .unwrap()
            .id;
        context.session = Some(session(&client, worktree).await.id);
        assert_eq!(
            client
                .execute(write(&client, &context, "wrong-session.txt"))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert!(!root.join("wrong-session.txt").exists());
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
