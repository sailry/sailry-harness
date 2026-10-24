use super::*;
use serde_json::json;

#[tokio::test]
async fn keeps_compatible_pdf_turns_usable() {
    for api in [ModelApi::Responses, ModelApi::ChatCompletions] {
        let fixture = Fixture::packages(&["files"]).await;
        for (index, client) in fixture.clients().into_iter().enumerate() {
            let bytes = include_bytes!("../../../src/office/fixtures/report.pdf");
            let name = if index == 0 {
                "report.pdf"
            } else {
                "报告.bin"
            };
            let pdf = upload(
                &client,
                fixture.worktree,
                name,
                if index == 0 {
                    "application/pdf"
                } else {
                    "application/octet-stream"
                },
                bytes,
            )
            .await;
            let tool = server::plugin_tool("files", "read_office");
            let arguments = json!({"source":{"kind":"attachment","value":pdf.id}});
            let server = if api == ModelApi::Responses {
                server::Server::phases_with_tool(&tool, arguments).await
            } else {
                server::Server::tools(vec![(tool.clone(), arguments)]).await
            };
            let (session, mut provider) =
                session(&client, fixture.worktree, &server.endpoint, api).await;
            assert!(provider.models[0].vision);
            provider.models[0].capabilities.as_mut().unwrap().tools = Some(true);
            client
                .execute(client.prepare(Command::PutProvider {
                    expected_revision: provider.revision,
                    provider,
                }))
                .await
                .unwrap();
            let turn = queue(&client, &session, input("Summarize the PDF", &[&pdf])).await;
            let capture = async |turn: TurnId| {
                let sql = ("SELECT r.body FROM requests r JOIN turns t ON t.request=r.id AND t.caller=r.caller WHERE t.id=?1").to_owned();
                let parameters = turso::params![turn.to_string()];
                fixture
                    .node
                    .with_storage(move |db| {
                        Box::pin(async move {
                            let row = db
                                .prepare(sql)
                                .await
                                .map_err(crate::database_fixture::fault)?
                                .query_row(parameters)
                                .await
                                .map_err(crate::database_fixture::fault)?;
                            let value = row.get::<Vec<u8>>(0);
                            value.map_err(crate::database_fixture::fault)
                        })
                    })
                    .await
                    .unwrap()
            };
            let sent = capture(turn.id).await;
            let original = run(&client, &turn).await;
            assert_eq!(
                original.runs[0].status,
                Status::Completed,
                "{:?}",
                original.runs
            );
            assert!(
                original
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| {
                        matches!(part, Part::ToolResult { name, result, .. }
                        if name == &tool && result["kind"] == "office_content"
                        && result["data"]["sections"].as_array().unwrap().iter().any(|section| {
                            section["text"].as_str().unwrap().contains("PDF acceptance")
                        }))
                    })
            );
            let next = queue(
                &client,
                &session,
                "Continue without another attachment".into(),
            )
            .await;
            let continued = run(&client, &next).await;
            assert_eq!(continued.runs.last().unwrap().status, Status::Completed);
            assert!(
                continued
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| { *part == Part::Attachment(pdf.clone()) })
            );
            assert_eq!(capture(turn.id).await, sent);
            assert_eq!(download(&client, &pdf).await, bytes);
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 3);
            for request in &requests {
                let messages = &request[if api == ModelApi::Responses {
                    "input"
                } else {
                    "messages"
                }];
                let has_file = messages
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|message| message["content"].as_array())
                    .flatten()
                    .any(|part| part["type"] == "input_file" || part["type"] == "file");
                assert!(!has_file);
                let serialized = request.to_string();
                assert!(serialized.contains(name));
                assert!(serialized.contains(&pdf.id.to_string()));
                assert!(serialized.contains("Contents not included; use read_office"));
                assert!(!serialized.contains("data:application/pdf;base64,"));
                assert!(!serialized.contains("sailry-attachment://"));
            }
            assert!(requests[2].to_string().contains("PDF acceptance"));
            assert!(
                requests[2]
                    .to_string()
                    .contains("Continue without another attachment")
            );
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn reads_visible_document_attachments() {
    let fixture = Fixture::packages(&["files"]).await;
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
            "chart.xlsx",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            include_bytes!("../../../src/office/fixtures/chart.xlsx"),
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
        let future = queue(&client, &session, input("Held future input", &[&foreign])).await;
        let history = run(&client, &turn).await;
        assert_eq!(
            history
                .runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Completed,
            "{:?}",
            history.runs
        );
        assert_eq!(
            history
                .runs
                .iter()
                .find(|run| run.turn == future.id)
                .unwrap()
                .status,
            Status::Queued
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
        for (result, text) in results
            .iter()
            .take(2)
            .zip(["PDF acceptance", "B4: 20 [=SUM(B2:B3)]"])
        {
            assert_eq!(result["kind"], "office_content");
            assert!(
                result["data"]["sections"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|section| { section["text"].as_str().unwrap().contains(text) })
            );
        }
        assert_eq!(results[1]["data"]["revision"], workbook.spec.revision);
        assert_eq!(
            download(&client, &workbook).await,
            include_bytes!("../../../src/office/fixtures/chart.xlsx")
        );
        assert_eq!(results[2]["error"]["code"], "not_found");
        client
            .execute(client.prepare(Command::StopTurn { turn: future.id }))
            .await
            .unwrap();
        let request = server.requests.lock().unwrap().first().cloned().unwrap();
        assert!(request.to_string().contains("read_office"));
        assert!(request.to_string().contains("Contents not included"));

        // A completed visible turn retains its attachment read authority.
        server.replace_tools(vec![(
            tool.clone(),
            json!({"source":{"kind":"attachment","value":pdf.id}}),
        )]);
        let next = queue(
            &client,
            &session,
            input("Try the previous attachment", &[&foreign]),
        )
        .await;
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
        assert_eq!(result["kind"], "office_content");
        assert!(
            result["data"]["sections"]
                .as_array()
                .unwrap()
                .iter()
                .any(|section| section["text"].as_str().unwrap().contains("PDF acceptance"))
        );

        let Output::Session(fork) = client
            .execute(client.prepare(Command::ForkConversation {
                session: session.id,
                through: turn.id,
                expected_revision: session.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("fork expected")
        };
        server.replace_tools(vec![
            (
                tool.clone(),
                json!({"source":{"kind":"attachment","value":pdf.id}}),
            ),
            (
                tool.clone(),
                json!({"source":{"kind":"attachment","value":foreign.id}}),
            ),
        ]);
        let fork_turn = queue(&client, &fork, "Read the retained attachment".into()).await;
        let fork_page = run(&client, &fork_turn).await;
        let fork_results: Vec<_> = fork_page
            .entries
            .iter()
            .filter(|entry| entry.turn == fork_turn.id)
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| {
                if let Part::ToolResult { result, .. } = part {
                    Some(result)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(fork_results.len(), 2);
        assert_eq!(fork_results[0]["kind"], "office_content");
        assert_eq!(fork_results[1]["error"]["code"], "not_found");
        server.replace_tools(vec![(
            tool.clone(),
            json!({"source":{"kind":"attachment","value":pdf.id}}),
        )]);

        let latest = page(&client, session.id).await;
        client
            .execute(client.prepare(Command::RewindConversation {
                session: session.id,
                through: None,
                expected_head: next.id,
                expected_revision: latest.revision,
            }))
            .await
            .unwrap();
        let removed = queue(&client, &session, "Read a removed attachment".into()).await;
        let removed_page = run(&client, &removed).await;
        assert!(removed_page.entries.iter().filter(|entry|entry.turn==removed.id)
            .flat_map(|entry|&entry.parts).any(|part|matches!(part,Part::ToolResult{result,..} if result["error"]["code"]=="not_found")));
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
