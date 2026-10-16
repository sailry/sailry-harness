#![cfg(unix)]
use super::*;
use serde_json::{Value, json};

#[tokio::test]
async fn lifecycle_keeps_definitions_and_configuration() {
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![]).await;
        let fixture = process::Fixture::packages(remote, &server, &["files"]).await;
        let Output::OcrModels(before) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadOcrModels))
            .await
            .unwrap()
        else {
            panic!("OCR models expected")
        };
        let id = before.first().unwrap().id.clone();
        assert!(
            before
                .iter()
                .all(|model| model.state == sailry_protocol::ocr::State::Missing)
        );
        let settings = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadOcrSettings))
            .await
            .unwrap();
        server.replace_tools(vec![
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
        let results: Vec<&Value> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| {
                if let Part::ToolResult { result, .. } = part {
                    Some(result)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(results.len(), 4, "{results:?}");
        assert_eq!(results[0]["models"].as_array().unwrap().len(), before.len());
        assert!(results[1].get("error").is_some(), "{results:?}");
        assert_eq!(results[2]["kind"], "ocr_model", "{results:?}");
        assert_eq!(results[2]["data"]["id"], id);
        assert_eq!(results[3]["models"], json!([]));
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadOcrSettings))
                .await
                .unwrap(),
            settings
        );
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadOcrModels))
                .await
                .unwrap(),
            Output::OcrModels(before)
        );
        let request = server.requests.lock().unwrap().first().cloned().unwrap();
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
                    .any(|tool| tool["function"]["name"] == plugin_tool("files", name))
            );
        }
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
