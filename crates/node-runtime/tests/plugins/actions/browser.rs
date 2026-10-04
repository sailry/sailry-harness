use super::*;
use sailry_protocol::browser::Action as BrowserAction;
use serde_json::json;

fn request(client: &Client, context: &Context, action: BrowserAction) -> Request {
    client
        .prepare(Command::UseBrowser {
            session: context.session.unwrap(),
            worktree: context.worktree.unwrap(),
            action,
        })
        .with_plugin(context.clone())
}

#[tokio::test]
async fn recovers_receipts_without_resending_controller_calls() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let mut context = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadBrowser, Action::ControlBrowser],
        )
        .await;
        context.session = Some(scopes::session(&client, worktree).await.id);
        let mut subscription = client.subscribe_browser().await.unwrap();
        let original = request(&client, &context, BrowserAction::Tabs);
        let pending = client.dispatch(original.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        drop(pending);
        let Update::BrowserCall(call) = subscription.next().await.unwrap() else {
            panic!("browser call expected")
        };
        assert_eq!(call.id, original.id);
        assert_eq!(Some(call.session), context.session);
        let expected = Output::Browser(json!({"tabs":[{"id":3,"url":"https://example.com"}]}));
        let Output::Browser(value) = &expected else {
            unreachable!()
        };
        execute(
            &client,
            Command::CompleteBrowser {
                id: call.id,
                result: Ok(value.clone()),
            },
        )
        .await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let RequestOutcome::Completed(result) = client.outcome(&original).await.unwrap()
                {
                    assert_eq!(result.unwrap(), expected);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let mut changed = original.clone();
        let Command::UseBrowser { action, .. } = &mut changed.command else {
            unreachable!()
        };
        *action = BrowserAction::Read { tab: Some(3) };
        assert_eq!(
            client.execute(changed).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "example".into(),
                expected_revision: 1,
                enabled: false,
            },
        )
        .await;
        assert_eq!(client.execute(original.clone()).await.unwrap(), expected);
        assert_eq!(
            client
                .execute(request(&client, &context, BrowserAction::Tabs))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(30), subscription.next())
                .await
                .is_err()
        );
        execute(
            &client,
            Command::RemovePlugin {
                name: "example".into(),
                expected_revision: 2,
            },
        )
        .await;
        drop(subscription);
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let mut subscription = client.subscribe_browser().await.unwrap();
        assert_eq!(client.execute(original).await.unwrap(), expected);
        assert!(
            tokio::time::timeout(Duration::from_millis(30), subscription.next())
                .await
                .is_err()
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::CompleteBrowser {
                    id: call.id,
                    result: Ok(json!({}))
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        drop(subscription);
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn distinguishes_grants_scope_and_uncertain_navigation() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let mut context = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadBrowser],
        )
        .await;
        context.session = Some(scopes::session(&client, worktree).await.id);
        for (action, code) in [
            (BrowserAction::Tabs, ErrorCode::Unavailable),
            (
                BrowserAction::Navigate {
                    url: "https://example.com".into(),
                    tab: None,
                },
                ErrorCode::OutcomeUnknown,
            ),
        ] {
            let mut subscription = client.subscribe_browser().await.unwrap();
            let original = request(&client, &context, action);
            let pending = client.dispatch(original.clone()).await.unwrap();
            let Update::BrowserCall(call) = subscription.next().await.unwrap() else {
                panic!()
            };
            assert_eq!(call.id, original.id);
            drop(subscription);
            let result = pending.completion.await.unwrap();
            assert_eq!(result.as_ref().unwrap_err().code, code);
            let mut subscription = client.subscribe_browser().await.unwrap();
            assert_eq!(client.execute(original.clone()).await, result);
            assert_eq!(
                client.outcome(&original).await.unwrap(),
                RequestOutcome::Completed(Box::new(result))
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(30), subscription.next())
                    .await
                    .is_err()
            );
            drop(subscription);
        }
        let mut subscription = client.subscribe_browser().await.unwrap();
        assert_eq!(
            client
                .execute(request(
                    &client,
                    &context,
                    BrowserAction::Click {
                        tab: 0,
                        snapshot: "fixture".into(),
                        element: 1
                    }
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        for field in ["session", "worktree"] {
            let mut outside = request(&client, &context, BrowserAction::Tabs);
            let Command::UseBrowser {
                session, worktree, ..
            } = &mut outside.command
            else {
                unreachable!()
            };
            if field == "session" {
                *session = SessionId::new()
            } else {
                *worktree = WorktreeId::new()
            }
            assert_eq!(
                client.execute(outside).await.unwrap_err().code,
                ErrorCode::PermissionDenied
            );
        }
        assert_eq!(
            client
                .execute(request(
                    &client,
                    &context,
                    BrowserAction::Navigate {
                        url: "file:///outside".into(),
                        tab: None
                    }
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(30), subscription.next())
                .await
                .is_err()
        );
        drop(subscription);
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
