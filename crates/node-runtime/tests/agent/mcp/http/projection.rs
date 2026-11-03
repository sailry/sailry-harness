use super::*;

fn projected(request: &Value) -> &str {
    let message = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "tool")
        .expect("MCP result must reach the model");
    message["content"].as_str().unwrap()
}

#[tokio::test]
async fn deduplicates_requests_and_preserves_history() {
    for remote in [false, true] {
        let http = Http::start(Reply::RepeatedJson).await;
        let name = alias("remote", "read");
        let text = "Complete result 中文 🙂\n".repeat(100);
        let output = json!({"value":text});
        let model = Server::tools(vec![(name.clone(), json!({"value":text}))]).await;
        let mut fixture = process::Fixture::packages(remote, &model, &[]).await;
        package(
            &fixture.root,
            json!({"remote":{"type":"streamable-http","url":http.endpoint}}),
            "repeated output",
        );
        install(&fixture, 0).await;
        let (_, turn) = submit(&fixture, false).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert!(page.approvals.is_empty());
        let canonical = results(&fixture.client, &page).await;
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0]["structuredContent"], output);
        assert_eq!(
            canonical[0]["content"],
            json!([
                {"type":"text","text":output.to_string()},
                {"type":"text","text":"Distinct report note"}
            ])
        );
        let expected = format!("{output} Distinct report note");
        assert_eq!(projected(&model.requests.lock().unwrap()[1]), expected);
        assert_eq!(http.count("tools/call"), 1);

        fixture.node.shutdown().await.unwrap();
        fixture.node = Node::start_empty(fixture.directory.path().join("node"))
            .await
            .unwrap();
        fixture.client = Client::new(if remote {
            fixture
                .controller
                .handle()
                .remote(fixture.node.link().address())
        } else {
            fixture.node.local()
        });
        assert_eq!(history(&fixture.client, fixture.session.id).await, page);
        let (_, turn) = submit(&fixture, false).await;
        let restored = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(restored.runs.last().unwrap().status, Status::Completed);
        assert_eq!(results(&fixture.client, &restored).await, canonical);
        assert_eq!(
            projected(model.requests.lock().unwrap().last().unwrap()),
            expected
        );
        assert_eq!(http.count("tools/call"), 1);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn pages_bounded_results_without_losing_history() {
    use sailry_protocol::tool::{MAX_OUTPUT_PAGE_BYTES, OutputReference};
    for remote in [false, true] {
        let http = Http::start(Reply::RepeatedJson).await;
        let name = alias("remote", "read");
        let text = "Original result 中文🙂\n".repeat(6000);
        let gate = Arc::new(tokio::sync::Notify::new());
        let model =
            Server::held_after_tool(vec![(name.clone(), json!({"value":text}))], gate.clone())
                .await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        package(
            &fixture.root,
            json!({"remote":{"type":"streamable-http","url":http.endpoint}}),
            "bounded output",
        );
        install(&fixture, 0).await;
        let mut provider = configuration::snapshot(&fixture.client).await.providers[0].clone();
        provider.models[0].context = 1_000_000;
        provider.models[0].compaction_tokens = Some(800_000);
        execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await;
        let (_, turn) = submit(&fixture, false).await;
        model.wait_count(2).await;
        let sent = projected(&model.requests.lock().unwrap()[1]).to_owned();
        assert!(sent.len() <= 50 * 1024);
        assert!(sent.contains("Tool result truncated"));
        let (_, reference) = sent.split_once("output=").unwrap();
        let (reference, _) = reference.split_once(" and offset=").unwrap();
        let reference: OutputReference = serde_json::from_str(reference).unwrap();
        let page = history(&fixture.client, fixture.session.id).await;
        let saved = results(&fixture.client, &page).await;
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0]["structuredContent"]["value"], text);
        let canonical = saved[0].to_string();
        assert_eq!(
            reference.revision,
            blake3::hash(canonical.as_bytes()).to_hex().to_string()
        );
        let mut offset = 0;
        let mut complete = String::new();
        loop {
            let Output::ToolOutput(page) = execute(
                &fixture.client,
                Command::ReadToolOutput {
                    turn: turn.id,
                    output: reference.clone(),
                    stream: None,
                    offset,
                    limit: MAX_OUTPUT_PAGE_BYTES,
                },
            )
            .await
            else {
                panic!("canonical result page expected")
            };
            assert_eq!(page.offset, offset);
            assert_eq!(page.total_bytes, canonical.len());
            assert_eq!(page.output, reference);
            complete.push_str(&page.text);
            let Some(next) = page.next_offset else {
                break;
            };
            assert!(next > offset && canonical.is_char_boundary(next));
            offset = next;
        }
        assert_eq!(complete, canonical);
        assert_eq!(http.count("tools/call"), 1);
        gate.notify_one();
        let finished = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(finished.runs.last().unwrap().status, Status::Completed);
        assert_eq!(results(&fixture.client, &finished).await, saved);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
