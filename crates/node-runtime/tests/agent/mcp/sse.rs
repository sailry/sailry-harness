use super::input as server;
use super::*;
use question::{Answer, Response, State};
use std::sync::atomic::Ordering;

#[tokio::test]
async fn runs_tools_and_forms() {
    for remote in [false, true] {
        let server = server::Server::with_transport(server::form_schema(), true).await;
        let model = Server::tools(vec![(alias("input", "read"), json!({})); 2]).await;
        let fixture = process::Fixture::new(remote, &model).await;
        server.package(&fixture.root.join("package"));
        let plugin = install(&fixture, 0).await;
        assert!(plugin.issues.is_empty());

        let (request, turn) = submit(&fixture, false).await;
        let (_, first) = questions::pending(&fixture.client, fixture.session.id).await;
        let answer = fixture.client.prepare(Command::ResolveQuestion {
            session: fixture.session.id,
            question: first.id,
            response: Response::Answer(Answer::Form(
                json!({
                    "title":"Legacy SSE", "copies":2, "ratio":1.5, "publish":false,
                    "format":"md", "sections":["summary"]
                })
                .as_object()
                .unwrap()
                .clone(),
            )),
        });
        let receipt = fixture.client.execute(answer.clone()).await.unwrap();
        let (_, second) = questions::pending(&fixture.client, fixture.session.id).await;
        execute(
            &fixture.client,
            Command::ResolveQuestion {
                session: fixture.session.id,
                question: second.id,
                response: Response::Decline,
            },
        )
        .await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert_eq!(page.questions[1].state, State::Declined);
        assert_eq!(fixture.client.execute(answer).await.unwrap(), receipt);
        assert_eq!(
            fixture.client.execute(request).await.unwrap(),
            Output::QueuedTurn(turn)
        );
        assert_eq!(server.calls.load(Ordering::SeqCst), 2);
        assert_eq!(model.requests.lock().unwrap().len(), 3);
        assert!(
            !serde_json::to_string(&*model.requests.lock().unwrap())
                .unwrap()
                .contains("mcp_elicitation")
        );
        assert!(
            server.streams.load(Ordering::SeqCst) > 0,
            "completed turns retain the session connection"
        );
        fixture.node.shutdown().await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while server.streams.load(Ordering::SeqCst) != 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("SSE transport must close at Node shutdown");
        fixture.controller.close().await.unwrap();
    }
}
