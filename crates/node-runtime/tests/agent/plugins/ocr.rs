#![cfg(unix)]
use super::*;
use serde_json::{Value, json};

#[tokio::test]
async fn lifecycle_keeps_definitions_and_configuration() {
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![]).await;
        let fixture = process::Fixture::packages(remote, &server, &["files"]).await;
        let Output::LocalModels(before) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadLocalModels {
                kind: Some(sailry_protocol::models::Kind::Ocr),
            }))
            .await
            .unwrap()
        else {
            panic!("OCR models expected")
        };
        let id = before.models.first().unwrap().id.clone();
        assert!(
            before
                .models
                .iter()
                .all(|model| model.state == sailry_protocol::models::State::Missing)
        );
        let settings = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadLocalModelSettings))
            .await
            .unwrap();
        server.replace_tools(vec![
            (
                "tool_search".into(),
                json!({"query":plugin_tool("files", "list_ocr_models"),"limit":1}),
            ),
            (
                "tool_search".into(),
                json!({"query":plugin_tool("files", "install_ocr_model"),"limit":1}),
            ),
            (
                "tool_search".into(),
                json!({"query":plugin_tool("files", "uninstall_ocr_model"),"limit":1}),
            ),
            (plugin_tool("files", "list_ocr_models"), json!({})),
            (plugin_tool("files", "install_ocr_model"), json!({"id":id})),
            (
                plugin_tool("files", "uninstall_ocr_model"),
                json!({"id":id}),
            ),
            (
                plugin_tool("files", "list_ocr_models"),
                json!({"installed_only":true}),
            ),
        ]);
        let turn = approvals::submit(&fixture.client, fixture.session.id).await;
        let (_, install) = approvals::pending(&fixture.client, fixture.session.id).await;
        process::decide(
            &fixture.client,
            &install,
            Decision::DenyWithReason("Keep models unchanged".into()),
        )
        .await;
        let (_, remove) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert_ne!(install.id, remove.id);
        process::decide(&fixture.client, &remove, Decision::Approve).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        let results: Vec<Value> = tool_details(&fixture.client, &page)
            .await
            .into_iter()
            .filter(|(name, _)| name != "tool_search")
            .map(|(_, detail)| detail.result.unwrap())
            .collect();
        assert_eq!(results.len(), 4, "{results:?}");
        assert_eq!(
            results[0]["models"].as_array().unwrap().len(),
            before.models.len()
        );
        assert_eq!(
            results[1],
            serde_json::to_value(rmcp::model::CallToolResult::error(vec![
                rmcp::model::ContentBlock::text(
                    "Tool execution was denied. Reason: Keep models unchanged"
                ),
            ]))
            .unwrap()
        );
        assert_eq!(results[2]["kind"], "local_model", "{results:?}");
        assert_eq!(results[2]["data"]["id"], id);
        assert_eq!(results[3]["models"], json!([]));
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadLocalModelSettings))
                .await
                .unwrap(),
            settings
        );
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadLocalModels {
                    kind: Some(sailry_protocol::models::Kind::Ocr)
                }))
                .await
                .unwrap(),
            Output::LocalModels(before)
        );
        let requests = server.requests.lock().unwrap().clone();
        let request = requests.last().unwrap();
        assert_eq!(requests.len(), 8);
        for name in [
            "list_ocr_models",
            "install_ocr_model",
            "uninstall_ocr_model",
        ] {
            assert!(
                request["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| tool["function"]["name"] == name)
            );
        }
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
