use super::*;
use serde_json::json;

fn declared(request: &Value, name: &str) -> bool {
    request["tools"].as_array().is_some_and(|tools| {
        tools
            .iter()
            .any(|tool| tool["function"]["name"] == exposed(name))
    })
}

async fn compact(client: &Client, session: SessionId) {
    let Output::QueuedTurn(turn) = client
        .execute(client.prepare(Command::CompactContext {
            focus: None,
            session,
            expected_revision: 1,
        }))
        .await
        .unwrap()
    else {
        panic!("compaction turn expected")
    };
    let page = finished(client, session, turn.id).await;
    let run = page.runs.iter().find(|run| run.turn == turn.id).unwrap();
    assert_eq!(run.status, Status::Completed, "{:?}", page.runs);
}

#[tokio::test]
async fn unloads_unused_discoveries() {
    for remote in [false, true] {
        let deferred = plugin_tool("files", "create_directory");
        let server = Server::scripted_tools(vec![(
            "tool_search".into(),
            json!({"query":deferred,"limit":1}),
        )])
        .await;
        let fixture = fixture(remote, &server).await;
        let session = fixture.session.id;
        let latest = || server.requests.lock().unwrap().last().unwrap().clone();
        submit(&fixture.client, session, "Find a directory tool".into()).await;
        let first = server.requests.lock().unwrap()[0].clone();
        assert!(!declared(&first, &deferred));
        assert!(declared(&latest(), &deferred));
        server.replace_tools(Vec::new());
        submit(&fixture.client, session, "Answer without tools".into()).await;
        assert!(declared(&latest(), &deferred));
        // The replaced history discovered the tool, so it stays loaded.
        compact(&fixture.client, session).await;
        submit(&fixture.client, session, "Continue after compaction".into()).await;
        assert!(declared(&latest(), &deferred));
        // Nothing used it since the previous compaction.
        compact(&fixture.client, session).await;
        submit(&fixture.client, session, "Continue again".into()).await;
        let request = latest();
        assert!(!declared(&request, &deferred));
        assert!(declared(&request, "tool_search"));
        // Discovery finds the unloaded tool again.
        server.replace_tools(vec![(
            "tool_search".into(),
            json!({"query":deferred,"limit":1}),
        )]);
        submit(
            &fixture.client,
            session,
            "Find the directory tool again".into(),
        )
        .await;
        assert!(declared(&latest(), &deferred));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
