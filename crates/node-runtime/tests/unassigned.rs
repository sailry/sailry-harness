use crate::agent_support;
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::*;
use std::time::Duration;

fn create() -> Command {
    Command::CreateSession {
        project: None,
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
    }
}

async fn snapshot(client: &Client) -> Snapshot {
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    snapshot
}

#[tokio::test]
async fn local_and_remote_lifecycle() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let profile = root.join("node");
        let node = Node::start(&profile).await.unwrap();
        let controller = Link::controller(root.join("controller"), NetworkScope::default())
            .await
            .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let request = client.prepare(create());
        let first = client.execute(request.clone()).await.unwrap();
        let Output::Session(session) = first.clone() else {
            panic!("session expected")
        };
        assert!(session.project.is_none());
        assert_eq!(client.execute(request.clone()).await.unwrap(), first);
        let state = snapshot(&client).await;
        assert!(state.projects.is_empty());
        assert_eq!(state.sessions.as_slice(), std::slice::from_ref(&session));
        assert_eq!(state.worktrees.len(), 1);
        let workspace = profile
            .join("workspaces/sessions")
            .join(session.id.to_string());
        assert_eq!(std::path::Path::new(&state.worktrees[0].path), workspace);
        assert!(
            !workspace.exists(),
            "chat creation must not create a directory"
        );

        let mut invalid = create();
        if let Command::CreateSession { worktree, .. } = &mut invalid {
            *worktree = Some(session.worktree);
        }
        assert_eq!(
            client
                .execute(client.prepare(invalid))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        let Output::Session(second) = client.execute(client.prepare(create())).await.unwrap()
        else {
            panic!("session expected")
        };
        assert_ne!(session.worktree, second.worktree);
        let second_workspace = profile
            .join("workspaces/sessions")
            .join(second.id.to_string());

        client
            .execute(client.prepare(Command::WriteFile {
                worktree: session.worktree,
                path: "output/note.txt".into(),
                text: "Keep this generated file".into(),
                expected_revision: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(workspace.join("output/note.txt")).unwrap(),
            "Keep this generated file"
        );
        assert!(!workspace.join(".git").exists());
        assert!(!second_workspace.exists());
        let Output::GitStatus(status) = client
            .execute(client.prepare(Command::InspectGit {
                worktree: session.worktree,
            }))
            .await
            .unwrap()
        else {
            panic!("Git status expected")
        };
        assert_eq!(status.kind, RepositoryKind::Directory);
        client
            .execute(client.prepare(Command::RemoveSession {
                session: session.id,
                expected_revision: session.revision,
            }))
            .await
            .unwrap();
        assert!(workspace.join("output/note.txt").is_file());
        assert_eq!(
            snapshot(&client).await.sessions.as_slice(),
            std::slice::from_ref(&second)
        );
        drop(client);
        node.shutdown().await.unwrap();

        let node = Node::start(&profile).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(snapshot(&client).await.sessions, [second]);
        assert_eq!(client.execute(request).await.unwrap(), first);
        assert!(workspace.join("output/note.txt").is_file());
        assert!(!second_workspace.exists());
        assert!(snapshot(&client).await.projects.is_empty());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn reports_usage_after_restart() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let profile = root.join("node");
        let node = Node::start(&profile).await.unwrap();
        let controller = Link::controller(root.join("controller"), NetworkScope::default())
            .await
            .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let server = agent_support::Server::start(false).await;
        let Output::Provider(provider) = client
            .execute(client.prepare(Command::PutProvider {
                provider: conversation::Provider {
                    oauth: None,
                    options: None,
                    id: ProviderId::new(),
                    revision: 0,
                    name: "Unassigned fixture".into(),
                    api: conversation::ModelApi::ChatCompletions,
                    authentication: Authentication::ApiKey,
                    endpoint: server.endpoint.clone(),
                    enabled: true,
                    models: Vec::new(),
                    default_model: String::new(),
                    credential: None,
                },
                expected_revision: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected");
        };
        let mut command = create();
        if let Command::CreateSession {
            config: Some(config),
            ..
        } = &mut command
        {
            config.provider = provider.id;
            config.model = "fixture-a".into();
        }
        let Output::Session(session) = client.execute(client.prepare(command)).await.unwrap()
        else {
            panic!("session expected");
        };
        let now = chrono::Utc::now().timestamp_millis();
        let query = usage::Query {
            start_ms: now - 86_400_000,
            end_ms: now + 86_400_000,
            dimension: usage::Dimension::Model,
            projects: Vec::new(),
            worktrees: Vec::new(),
            providers: Vec::new(),
            models: Vec::new(),
            before: None,
        };
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Usage without a project".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected");
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let Output::Conversation(history) = client
                    .execute(client.prepare(Command::ReadConversation {
                        session: session.id,
                        before: None,
                        limit: 100,
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("history expected");
                };
                if let Some(run) = history.page.runs.iter().find(|run| run.turn == turn.id)
                    && !matches!(
                        run.status,
                        conversation::Status::Queued
                            | conversation::Status::Running
                            | conversation::Status::Stopping
                    )
                {
                    assert_eq!(run.status, conversation::Status::Completed);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Agent completion deadline");
        let report = client.read_usage(query.clone()).await.unwrap();
        assert_eq!(report.totals.responses, 1);
        assert_eq!(
            report.totals.tokens,
            Some(conversation::Usage {
                input: 12,
                output: 4,
                cached_input: 0,
                reasoning: 0,
            })
        );
        let model = usage::Key::Model {
            provider: provider.id,
            model: "fixture-a".into(),
        };
        assert_eq!(
            report.resources,
            [
                usage::Key::Provider(provider.id),
                model.clone(),
                usage::Key::Session(session.id),
            ]
        );
        assert_eq!(report.groups.len(), 1);
        assert_eq!(report.groups[0].key, model);
        assert_eq!(report.requests.items.len(), 1);
        let call = &report.requests.items[0];
        assert_eq!(call.session, session.id);
        assert_eq!(call.turn, turn.id);
        assert_eq!(call.project, None);
        assert_eq!(call.worktree, session.worktree);
        assert_eq!(call.provider, provider.id);
        assert_eq!(call.model, "fixture-a");
        assert_eq!(call.scope_name, "");
        let workspace = profile
            .join("workspaces/sessions")
            .join(session.id.to_string());
        assert!(workspace.is_dir());
        assert!(std::fs::read_dir(&workspace).unwrap().next().is_none());
        let state = snapshot(&client).await;
        assert!(state.projects.is_empty());
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].id, session.id);
        assert_eq!(state.sessions[0].project, None);
        assert_eq!(state.sessions[0].worktree, session.worktree);
        assert_eq!(state.sessions[0].config, session.config);
        drop(client);
        node.shutdown().await.unwrap();

        let node = Node::start(&profile).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(client.read_usage(query).await.unwrap(), report);
        assert_eq!(snapshot(&client).await.sessions, state.sessions);
        assert!(snapshot(&client).await.projects.is_empty());
        assert!(workspace.is_dir());
        assert!(std::fs::read_dir(&workspace).unwrap().next().is_none());
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        drop(client);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
