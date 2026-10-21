use super::*;

#[tokio::test]
async fn preserves_management_branches_on_wire() {
    for remote in [false, true] {
        let server = Server::start(ModelApi::Anthropic, Reply::Text).await;
        let fixture = Fixture::empty(remote, ModelApi::Anthropic, &server.endpoint).await;
        let turn = fixture.submit("Say hello briefly").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            for name in ["manage_mcp", "manage_plugins", "manage_skills"] {
                let tool = requests[0].body["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|tool| tool["name"] == name)
                    .unwrap();
                let schema = &tool["input_schema"];
                assert_eq!(schema["type"], "object");
                assert_eq!(schema["required"], serde_json::json!(["action"]));
                assert!(schema["properties"]["action"]["enum"].is_array());
                for keyword in ["oneOf", "allOf", "anyOf"] {
                    assert!(schema.get(keyword).is_none(), "{name}: {keyword}");
                }
                assert!(schema["not"]["not"]["oneOf"].is_array());
            }
        }
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
