use super::*;

#[tokio::test]
async fn continues_with_separate_recommendations() {
    for remote in [false, true] {
        let recommended = "Keep the current layout (Recommended)";
        let server = Server::tools(vec![
            (
                "ask_user".into(),
                json!({"prompt":"Choose a layout", "input":{
                    "kind":"choice", "options":[recommended,"Use a compact layout"], "multiple":false
                }}),
            ),
            ("ask_user".into(), text_input()),
        ])
        .await;
        let fixture = process::Fixture::packages(remote, &server, &[]).await;
        fixture
            .node
            .set_question_timeout_fixture(Duration::from_millis(100));
        let session = fixture.session.id;
        let mut stream = fixture
            .client
            .subscribe_conversation(session)
            .await
            .unwrap();
        let mut projection =
            sailry_client::conversation::Projection::new(fixture.client.target(), session, 0);
        projection.apply(0, stream.next().await.unwrap()).unwrap();
        let turn = approvals::submit(&fixture.client, session).await;
        let page = finished(&fixture.client, session, turn).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert_eq!(page.questions.len(), 2);
        assert!(
            page.questions
                .iter()
                .all(|question| question.state == State::TimedOut)
        );
        assert!(page.approvals.is_empty());
        let values = results(&fixture.client, &page).await;
        assert_eq!(
            values,
            vec![
                json!({"status":"timed_out", "source":"automatic_timeout", "question":page.questions[0].id,
                "answers":[], "recommendations":[{"index":0,"option":recommended}]}),
                json!({"status":"timed_out", "source":"automatic_timeout", "question":page.questions[1].id,
                "answers":[], "recommendations":[]}),
            ]
        );
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        tokio::time::timeout(Duration::from_secs(10), async {
            while !projection
                .snapshot()
                .unwrap()
                .page
                .runs
                .last()
                .is_some_and(|run| run.status == Status::Completed)
            {
                projection.apply(0, stream.next().await.unwrap()).unwrap();
            }
        })
        .await
        .expect("question completion deadline");
        assert_eq!(*projection.snapshot().unwrap().page, page);
        let stored = serde_json::to_value(&page.questions).unwrap();
        assert!(
            stored
                .as_array()
                .unwrap()
                .iter()
                .all(|question| question["state"] == json!({"kind":"timed_out"}))
        );
        drop(stream);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
        let node = Node::start_empty(fixture.directory.path().join("node"))
            .await
            .unwrap();
        assert_eq!(history(&Client::new(node.local()), session).await, page);
        node.shutdown().await.unwrap();
    }
}
