use super::*;
use serde_json::json;

#[tokio::test]
async fn reads_only_current_document_attachments() {
    let fixture = Fixture::start().await;
    let server = server::Server::scripted_tools(vec![]).await;
    for client in fixture.clients() {
        let (session, mut provider) = session(
            &client,
            fixture.worktree,
            &server.endpoint,
            ModelApi::ChatCompletions,
        )
        .await;
        provider.models[0].tools = true;
        provider.models[0].vision = false;
        let capabilities = provider.models[0].capabilities.as_mut().unwrap();
        capabilities.tools = Some(true);
        capabilities.vision = Some(false);
        client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        let pdf = upload(
            &client,
            fixture.worktree,
            "report.pdf",
            "application/pdf",
            include_bytes!("../../../src/office/fixtures/report.pdf"),
        )
        .await;
        let workbook = upload(
            &client,
            fixture.worktree,
            "book.xlsx",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            include_bytes!("../../../src/office/fixtures/book.xlsx"),
        )
        .await;
        let foreign = upload(
            &client,
            fixture.worktree,
            "unbound.pdf",
            "application/pdf",
            include_bytes!("../../../src/office/fixtures/report.pdf"),
        )
        .await;
        let tool = server::plugin_tool("files", "read_office");
        server.replace_tools(
            [&pdf, &workbook, &foreign]
                .into_iter()
                .map(|attachment| {
                    (
                        tool.clone(),
                        json!({"source":{"kind":"attachment","value":attachment.id}}),
                    )
                })
                .collect(),
        );
        let turn = queue(
            &client,
            &session,
            input("Inspect the attached documents", &[&pdf, &workbook]),
        )
        .await;
        let history = run(&client, &turn).await;
        assert_eq!(
            history.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            history.runs
        );
        let results: Vec<_> = history
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::ToolResult { name, result, .. } => {
                    assert_eq!(name, &tool);
                    Some(result)
                }
                _ => None,
            })
            .collect();
        assert_eq!(results.len(), 3);
        for (result, text) in results.iter().take(2).zip(["PDF acceptance", "20"]) {
            assert_eq!(result["kind"], "office_content");
            assert!(
                result["data"]["sections"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|section| { section["text"].as_str().unwrap().contains(text) })
            );
        }
        assert_eq!(results[2]["error"]["code"], "not_found");
        let request = server.requests.lock().unwrap().first().cloned().unwrap();
        assert!(request.to_string().contains("read_office"));
        assert!(request.to_string().contains("Contents not included"));

        // A visible attachment in earlier history cannot authorize a new tool read.
        server.replace_tools(vec![(
            tool.clone(),
            json!({"source":{"kind":"attachment","value":pdf.id}}),
        )]);
        let next = queue(&client, &session, "Try the previous attachment".into()).await;
        let history = run(&client, &next).await;
        let result = history
            .entries
            .iter()
            .filter(|entry| entry.turn == next.id)
            .flat_map(|entry| &entry.parts)
            .find_map(|part| match part {
                Part::ToolResult { result, .. } => Some(result),
                _ => None,
            })
            .unwrap();
        assert_eq!(result["error"]["code"], "not_found");
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadOffice {
                    worktree: fixture.worktree,
                    options: sailry_protocol::office::Read {
                        source: sailry_protocol::office::Source::Attachment(pdf.id),
                        offset: 0,
                    },
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
    }
    fixture.close().await;
}
