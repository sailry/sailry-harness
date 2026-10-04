use super::*;
use sailry_protocol::activity;

fn request(client: &Client, context: &Context, command: Command) -> Request {
    client.prepare(command).with_plugin(context.clone())
}

#[tokio::test]
async fn catalog_matches_canonical_resources() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let mut context = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadActivity],
        )
        .await;
        context.worktree = None;
        let first = scopes::session(&client, worktree).await;
        let other_root = directory.path().join("other");
        fs::create_dir(&other_root).unwrap();
        let Output::Project(other) = execute(
            &client,
            Command::RegisterProject {
                name: "Other activity project".into(),
                path: other_root.to_str().unwrap().into(),
            },
        )
        .await
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let other_worktree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.project == Some(other.id))
            .unwrap()
            .id;
        let second = scopes::session(&client, other_worktree).await;
        #[cfg(unix)]
        {
            use sailry_protocol::terminal::{Appearance, ColorScheme, Launch, Rgb, Viewport};
            let Output::Terminal(terminal) = execute(
                &client,
                Command::CreateTerminal(Launch {
                    worktree,
                    viewport: Viewport {
                        columns: 80,
                        rows: 24,
                        pixel_width: 0,
                        pixel_height: 0,
                    },
                    appearance: Appearance {
                        foreground: Rgb {
                            red: 240,
                            green: 240,
                            blue: 240,
                        },
                        background: Rgb {
                            red: 20,
                            green: 20,
                            blue: 20,
                        },
                        palette: [Rgb {
                            red: 128,
                            green: 128,
                            blue: 128,
                        }; 16],
                        color_scheme: ColorScheme::Dark,
                    },
                }),
            )
            .await
            else {
                panic!("terminal expected")
            };
            execute(
                &client,
                Command::CloseTerminal {
                    worktree,
                    terminal: terminal.id,
                },
            )
            .await;
        }
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let pending = client
            .dispatch(request(&client, &context, Command::ReadActivityCatalog))
            .await
            .unwrap();
        assert!(!pending.receipt.durable);
        let Output::ActivityCatalog(catalog) = pending.completion.await.unwrap().unwrap() else {
            panic!("activity catalog expected")
        };
        assert_eq!(catalog.node, snapshot.node);
        assert_eq!(catalog.cursor, snapshot.cursor);
        assert_eq!(catalog.projects, snapshot.projects);
        assert_eq!(catalog.worktrees, snapshot.worktrees);
        assert_eq!(catalog.sessions, snapshot.sessions);
        assert_eq!(catalog.terminals, snapshot.terminals);
        assert_eq!(catalog.projects.len(), 2);
        assert!(
            catalog
                .sessions
                .iter()
                .any(|session| session.id == first.id)
        );
        assert!(
            catalog
                .sessions
                .iter()
                .any(|session| session.id == second.id)
        );
        #[cfg(unix)]
        assert_eq!(catalog.terminals.len(), 1);
        assert!(serde_json::to_vec(&catalog).unwrap().len() <= activity::MAX_CATALOG_BYTES);
        assert_eq!(
            execute(&client, Command::Snapshot).await,
            Output::Snapshot(snapshot)
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn capability_is_explicit_and_revalidated() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let package = directory.path().join("source/package");
        let mut previous =
            install_actions(&client, &package, worktree, 0, &[Action::ReadConversation]).await;
        previous.worktree = None;
        let session = scopes::session(&client, worktree).await;
        let commands = [
            Command::ReadActivityCatalog,
            Command::ReadActivity {
                sessions: vec![session.id],
            },
        ];
        for command in &commands {
            assert_eq!(
                client
                    .execute(request(&client, &previous, command.clone()))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        let mut context =
            install_actions(&client, &package, worktree, 1, &[Action::ReadActivity]).await;
        context.worktree = None;
        assert_ne!(previous.package.digest, context.package.digest);
        let mut scopes = [
            context.clone(),
            context.clone(),
            context.clone(),
            context.clone(),
        ];
        scopes[0].worktree = Some(worktree);
        scopes[1].session = Some(session.id);
        scopes[2].turn = Some(TurnId::new());
        scopes[3].invocation = Some(RequestId::new());
        for command in &commands {
            assert_eq!(
                client
                    .execute(request(&client, &previous, command.clone()))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::RevisionConflict
            );
            for scoped in &scopes {
                assert_eq!(
                    client
                        .execute(request(&client, scoped, command.clone()))
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::PermissionDenied
                );
            }
        }
        let mut history = context.clone();
        history.worktree = Some(worktree);
        history.session = Some(session.id);
        for command in [
            Command::ReadSession {
                session: session.id,
            },
            Command::ReadConversation {
                session: session.id,
                before: None,
                limit: 10,
            },
            Command::Snapshot,
        ] {
            assert_eq!(
                client
                    .execute(request(&client, &history, command))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        let disabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: "example".into(),
                    expected_revision: 2,
                    enabled: false,
                },
            )
            .await,
        );
        for command in &commands {
            assert_eq!(
                client
                    .execute(request(&client, &context, command.clone()))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotConfigured
            );
        }
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "example".into(),
                expected_revision: disabled.summary.revision,
                enabled: true,
            },
        )
        .await;
        for command in commands {
            let expected = execute(&client, command.clone()).await;
            assert_eq!(
                client
                    .execute(request(&client, &context, command))
                    .await
                    .unwrap(),
                expected
            );
        }
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn previews_are_bounded_without_exposing_history() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let mut context = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadActivity],
        )
        .await;
        context.worktree = None;
        let text = "Activity 中文 🙂 ".repeat(80);
        let server = model_fixture::Server::markdown(text.clone()).await;
        let model = completion::configure(&client, server.endpoint.clone()).await;
        let Output::Session(session) = execute(
            &client,
            Command::CreateSession {
                project: None,
                worktree: None,
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: model.split_once('/').unwrap().0.parse().unwrap(),
                    model: "fixture".into(),
                    effort: Effort::Default,
                    mode: WorkMode::Code,
                    permission: Permission::Ask,
                    credential: None,
                }),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        let Output::QueuedTurn(turn) = execute(
            &client,
            Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Create a public activity preview".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let Output::Session(session) = execute(
                    &client,
                    Command::ReadSession {
                        session: session.id,
                    },
                )
                .await
                else {
                    panic!("session expected")
                };
                if session.activity.run.as_ref().is_some_and(|run| {
                    run.turn == turn.id && run.status == conversation::Status::Completed
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("activity completion deadline");
        let command = Command::ReadActivity {
            sessions: vec![session.id],
        };
        let expected = execute(&client, command.clone()).await;
        let pending = client
            .dispatch(request(&client, &context, command))
            .await
            .unwrap();
        assert!(!pending.receipt.durable);
        let output = pending.completion.await.unwrap().unwrap();
        assert_eq!(output, expected);
        let Output::Activity(previews) = output else {
            panic!("activity previews expected")
        };
        assert_eq!(
            previews,
            vec![activity::Preview {
                session: session.id,
                turn: Some(turn.id),
                text: Some(text.chars().take(512).collect()),
            }]
        );
        assert_eq!(
            client
                .execute(request(
                    &client,
                    &context,
                    Command::ReadActivity {
                        sessions: vec![session.id; 33],
                    }
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            client
                .execute(request(
                    &client,
                    &context,
                    Command::ReadActivity {
                        sessions: vec![SessionId::new()],
                    }
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            client
                .execute(request(
                    &client,
                    &context,
                    Command::ReadConversation {
                        session: session.id,
                        before: None,
                        limit: 10,
                    }
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
