//! Metadata reads use the same authenticated local and remote service boundary.
use crate::file_fixture::Fixture;
use sailry_protocol::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pages_and_checks_scoped_membership() {
    let fixture = Fixture::packages(&["files", "goals"]).await;
    for client in fixture.clients() {
        let Output::ProjectCatalog(catalog) = client
            .execute(client.prepare(Command::ReadWorktreeCatalog {
                worktree: fixture.worktree,
            }))
            .await
            .unwrap()
        else {
            panic!("project catalog expected")
        };
        let project = catalog
            .worktrees
            .iter()
            .find(|worktree| worktree.id == fixture.worktree)
            .and_then(|worktree| worktree.project)
            .expect("file fixture worktree belongs to its registered project");
        let Output::Session(session) = client
            .execute(client.prepare(Command::CreateSession {
                project: Some(project),
                worktree: Some(fixture.worktree),
                config: Some(SessionConfig {
                    features: Default::default(),
                    assistant: None,
                    resource: None,
                    provider: ProviderId::new(),
                    model: "fixture".into(),
                    effort: Effort::Low,
                    mode: WorkMode::Code,
                    options: Default::default(),
                    permission: Permission::Ask,
                    credential: None,
                }),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        client
            .execute(client.prepare(Command::SetQueuePaused {
                session: session.id,
                expected_revision: 0,
                paused: true,
            }))
            .await
            .unwrap();
        let mut turns = Vec::new();
        for _ in 0..3 {
            let Output::QueuedTurn(turn) = client
                .execute(client.prepare(Command::QueueTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: "Private metadata fixture input".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("queued turn expected")
            };
            turns.push(turn.id);
        }
        let Output::ConversationRuns(page) = client
            .execute(client.prepare(Command::ReadConversationRuns {
                session: session.id,
                before: None,
                limit: 2,
            }))
            .await
            .unwrap()
        else {
            panic!("metadata page expected")
        };
        assert_eq!(
            page.runs.iter().map(|run| run.turn).collect::<Vec<_>>(),
            turns[1..]
        );
        assert_eq!(page.next_before, Some(turns[1]));
        assert_eq!(page.runs[0].status, conversation::Status::Queued);
        assert!(!serde_json::to_string(&page).unwrap().contains("Private"));
        let Output::ConversationRuns(older) = client
            .execute(client.prepare(Command::ReadConversationRuns {
                session: session.id,
                before: page.next_before,
                limit: 100,
            }))
            .await
            .unwrap()
        else {
            panic!("metadata page expected")
        };
        assert_eq!(older.runs[0].turn, turns[0]);
        assert_eq!(older.next_before, None);
        let command = Command::ReadConversationRun {
            session: session.id,
            turn: turns[0],
            expected_revision: Some(page.revision),
        };
        assert_eq!(
            client
                .execute(client.prepare(command.clone()))
                .await
                .unwrap(),
            Output::ConversationRun(older.runs[0].clone())
        );
        let stale = Command::ReadConversationRun {
            session: session.id,
            turn: turns[0],
            expected_revision: Some(page.revision + 1),
        };
        assert_eq!(
            client
                .execute(client.prepare(stale))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        for (name, allowed) in [("goals", true), ("files", false)] {
            let Output::Plugin(package) = client
                .execute(client.prepare(Command::ReadPlugin { name: name.into() }))
                .await
                .unwrap()
            else {
                panic!("package expected")
            };
            let Output::Plugin(package) = client
                .execute(client.prepare(Command::SetPluginEnabled {
                    name: name.into(),
                    expected_revision: package.summary.revision,
                    enabled: true,
                }))
                .await
                .unwrap()
            else {
                panic!("package expected")
            };
            let context = plugin::Context {
                invocation: None,
                turn: None,
                surface: plugin::desktop::Surface::Workspace,
                package: package.summary.reference(),
                worktree: Some(session.worktree),
                session: Some(session.id),
            };
            let result = client
                .execute(client.prepare(command.clone()).with_plugin(context.clone()))
                .await;
            if allowed {
                assert!(result.is_ok(), "{result:?}");
            } else {
                assert_eq!(result.unwrap_err().code, ErrorCode::PermissionDenied);
            }
            let mismatch = client
                .execute(
                    client
                        .prepare(Command::ReadConversationRuns {
                            session: SessionId::new(),
                            before: None,
                            limit: 100,
                        })
                        .with_plugin(context),
                )
                .await
                .unwrap_err();
            assert!(matches!(
                mismatch.code,
                ErrorCode::PermissionDenied | ErrorCode::WrongTarget
            ));
        }
    }
    fixture.close().await;
}
