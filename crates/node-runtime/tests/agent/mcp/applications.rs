use super::*;
use plugin::connector::Kind;
#[path = "../../support/connectors.rs"]
mod connectors;
use std::sync::atomic::Ordering;
#[path = "../../support/application_tokens.rs"]
mod issuer;

#[tokio::test]
async fn refreshes_persistent_application_transport() {
    for remote in [false, true] {
        let peer = input::Server::url("https://example.invalid/continue", false).await;
        let issuer = issuer::Server::start(Some(&peer.endpoint)).await;
        let model = Server::turn_tools(vec![(alias("input", "read"), json!({}))]).await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        fixture
            .node
            .set_connector_fixture(connectors::ORIGIN, &issuer.origin);
        package(
            &fixture.root,
            json!({"input":{"type":"streamable-http","url":format!("{}/mcp",connectors::ORIGIN),"headers":{"X-Access-Token":""}}}),
            "0.1.0",
        );
        connectors::configure(&fixture.root.join("package"), connectors::application(true));
        let package = install(&fixture, 0).await;
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        connectors::connected(
            &fixture.client,
            connectors::start(
                &fixture.client,
                package.summary.reference(),
                Kind::Application,
                false,
            ),
        )
        .await;
        for index in 0..2 {
            if index > 0 {
                tokio::time::sleep(Duration::from_millis(2100)).await;
            }
            let (_, turn) = submit(&fixture, false).await;
            let (_, pending) = questions::pending(&fixture.client, fixture.session.id).await;
            execute(
                &fixture.client,
                Command::ResolveQuestion {
                    session: fixture.session.id,
                    question: pending.id,
                    response: question::Response::Decline,
                },
            )
            .await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            assert!(
                !serde_json::to_string(&page)
                    .unwrap()
                    .contains(issuer::SECRET)
            );
        }
        assert_eq!(peer.initializations.load(Ordering::SeqCst), 1);
        assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
        assert_eq!(issuer.tokens.load(Ordering::SeqCst), 2);
        assert!(!issuer.failed.load(Ordering::SeqCst));
        {
            let records = issuer.records.lock().unwrap();
            let calls = records
                .iter()
                .filter(|(_, _, body)| body["method"] == "tools/call")
                .collect::<Vec<_>>();
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[0].1["x-access-token"], "application-token-1");
            assert_eq!(calls[1].1["x-access-token"], "application-token-2");
        }
        assert!(
            !serde_json::to_string(&*model.requests.lock().unwrap())
                .unwrap()
                .contains(issuer::SECRET)
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
