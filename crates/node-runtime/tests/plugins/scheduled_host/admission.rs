use super::*;

#[tokio::test]
async fn start_captures_defaults_once() {
    for remote in [false, true] {
        let server = model_fixture::Server::markdown("Started once".into()).await;
        let (directory, node, controller, client, tree) = fixture(remote).await;
        let context = installed(&client, directory.path(), tree).await;
        let configuration = config(&client, server.endpoint.clone()).await;
        execute(
            &client,
            Command::SetDefaults {
                expected_revision: 0,
                config: configuration.clone(),
            },
        )
        .await;
        let Output::ProjectCatalog(catalog) = execute(&client, Command::ReadProjectCatalog).await
        else {
            panic!("catalog expected")
        };
        let mut draft = conversation::Start {
            project: Some(catalog.projects[0].id),
            worktree: Some(tree),
            config: None,
            title: "Atomic start".into(),
            message: conversation::Input::default(),
        };
        let invalid = client
            .prepare(Command::StartSession(draft.clone()))
            .with_plugin(context.clone());
        assert_eq!(
            client.execute(invalid.clone()).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            client.execute(invalid).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert!(snapshot.sessions.is_empty());
        assert!(server.requests.lock().unwrap().is_empty());

        draft.message = "Start the fixture".to_owned().into();
        let mut scoped = context.clone();
        scoped.worktree = Some(WorktreeId::new());
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::StartSession(draft.clone()))
                        .with_plugin(scoped)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let mut agent = context.clone();
        agent.turn = Some(TurnId::new());
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::StartSession(draft.clone()))
                        .with_plugin(agent)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let mut ungranted = context.clone();
        ungranted.package = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "reminders".into(),
                },
            )
            .await,
        )
        .summary
        .reference();
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::StartSession(draft.clone()))
                        .with_plugin(ungranted)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );

        let mut scoped = context.clone();
        scoped.worktree = Some(tree);
        let request = client
            .prepare(Command::StartSession(draft))
            .with_plugin(scoped);
        let output = client.execute(request.clone()).await.unwrap();
        assert_eq!(client.execute(request).await.unwrap(), output);
        let Output::QueuedTurn(turn) = output else {
            panic!("turn expected")
        };
        let Output::Session(session) = execute(
            &client,
            Command::ReadSession {
                session: turn.session,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        assert_eq!(session.config, configuration);
        assert_eq!(session.project, Some(catalog.projects[0].id));
        assert_eq!(session.worktree, tree);
        server.wait_count(1).await;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let Output::Conversation(history) = execute(
                    &client,
                    Command::ReadConversation {
                        session: turn.session,
                        before: None,
                        limit: 10,
                    },
                )
                .await
                else {
                    panic!("history expected")
                };
                if history.page.runs[0].status == conversation::Status::Completed {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
