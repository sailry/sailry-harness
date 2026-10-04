use super::*;
use sailry_protocol::external_browser::Action as BrowserAction;
use serde_json::json;

fn command(session: SessionId, worktree: WorktreeId, action: BrowserAction) -> Command {
    Command::UseExternalBrowser {
        session,
        worktree,
        action,
        arguments: json!({}),
    }
}

#[tokio::test]
async fn scopes_control_and_recovers_receipts() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let mut context = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadExternalBrowser, Action::ControlExternalBrowser],
        )
        .await;
        let session = scopes::session(&client, worktree).await;
        let other = scopes::session(&client, worktree).await;
        context.session = Some(session.id);
        for (selected, tree) in [(other.id, worktree), (session.id, WorktreeId::new())] {
            let request = client
                .prepare(command(selected, tree, BrowserAction::CloseSession))
                .with_plugin(context.clone());
            let result = client.execute(request.clone()).await;
            assert_eq!(
                result.as_ref().unwrap_err().code,
                ErrorCode::PermissionDenied
            );
            assert_eq!(
                client.outcome(&request).await.unwrap(),
                RequestOutcome::Completed(Box::new(result))
            );
        }
        let request = client
            .prepare(command(session.id, worktree, BrowserAction::CloseSession))
            .with_plugin(context.clone());
        let pending = client.dispatch(request.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        drop(pending);
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let RequestOutcome::Completed(result) = client.outcome(&request).await.unwrap() {
                    break result.unwrap();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            result,
            Output::ExternalBrowser(json!({"closed":true,"execution_node":node.id()}))
        );
        assert_eq!(client.execute(request.clone()).await.unwrap(), result);
        let mut changed = request.clone();
        let Command::UseExternalBrowser { session, .. } = &mut changed.command else {
            panic!()
        };
        *session = other.id;
        assert_eq!(
            client.execute(changed.clone()).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        assert_eq!(
            client.outcome(&changed).await.unwrap_err().code,
            ErrorCode::Conflict
        );

        let disabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: context.package.name.clone(),
                    expected_revision: 1,
                    enabled: false,
                },
            )
            .await,
        );
        assert_eq!(client.execute(request.clone()).await.unwrap(), result);
        let fresh = client
            .prepare(command(
                context.session.unwrap(),
                worktree,
                BrowserAction::CloseSession,
            ))
            .with_plugin(context.clone());
        assert_eq!(
            client.execute(fresh.clone()).await.unwrap_err().code,
            ErrorCode::NotConfigured
        );
        execute(
            &client,
            Command::RemovePlugin {
                name: context.package.name.clone(),
                expected_revision: disabled.summary.revision,
            },
        )
        .await;
        let removed = client
            .prepare(command(
                context.session.unwrap(),
                worktree,
                BrowserAction::CloseSession,
            ))
            .with_plugin(context);
        assert_eq!(
            client.execute(removed).await.unwrap_err().code,
            ErrorCode::NotFound
        );
        assert!(
            !node.profile().join("browser").exists(),
            "closing an unopened browser must not provision Chromium"
        );
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(client.execute(request).await.unwrap(), result);
        assert_eq!(
            client.execute(fresh).await.unwrap_err().code,
            ErrorCode::NotConfigured
        );
        assert!(!node.profile().join("browser").exists());
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn checks_grants_and_arguments_before_provisioning() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let mut context = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadExternalBrowser],
        )
        .await;
        let session = scopes::session(&client, worktree).await;
        context.session = Some(session.id);
        let denied = client
            .prepare(command(session.id, worktree, BrowserAction::CloseSession))
            .with_plugin(context.clone());
        assert_eq!(
            client.execute(denied).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        for (action, arguments) in [
            (BrowserAction::Navigate, json!({"url":"file:///outside"})),
            (
                BrowserAction::Navigate,
                json!({"url":"https://user:secret@example.com"}),
            ),
            (BrowserAction::Wait, json!({"seconds":31})),
            (BrowserAction::Wait, json!({"seconds":-1})),
            (BrowserAction::Wait, json!([])),
        ] {
            let request = client
                .prepare(Command::UseExternalBrowser {
                    session: session.id,
                    worktree,
                    action,
                    arguments,
                })
                .with_plugin(context.clone());
            let result = client.execute(request.clone()).await;
            assert_eq!(result.as_ref().unwrap_err().code, ErrorCode::InvalidRequest);
            assert_eq!(
                client.outcome(&request).await.unwrap(),
                RequestOutcome::Completed(Box::new(result))
            );
        }
        // The service validates ordinary callers too; package checks are additional grants.
        assert_eq!(
            client
                .execute(client.prepare(command(
                    session.id,
                    WorktreeId::new(),
                    BrowserAction::CloseSession
                )))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert!(!node.profile().join("browser").exists());
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
