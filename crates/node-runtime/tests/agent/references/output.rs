use super::*;
use sailry_protocol::tool::{OutputPage, OutputReference};

#[tokio::test]
async fn pages_prior_results_without_command_tools() {
    for remote in [false, true] {
        let file = plugin_tool("files", "read_file");
        let server = Server::scripted_tools(vec![(
            file.clone(),
            json!({"path":"source.txt","limit":100}),
        )])
        .await;
        let fixture = process::Fixture::files(remote, &server).await;
        let text = format!(
            "\n  trailing spaces  \r\nFirst marker {}\r\n\t中文🙂\r\nLast marker",
            "neutral data ".repeat(240)
        );
        std::fs::write(fixture.root.join("source.txt"), &text).unwrap();
        let source = submit(
            &fixture.client,
            &fixture.session,
            "Read the source file".into(),
        )
        .await;
        let page = history(&fixture.client, fixture.session.id).await;
        let original = tool_details(&fixture.client, &page)
            .await
            .into_iter()
            .find(|(name, _)| name == &file)
            .unwrap()
            .1
            .result
            .unwrap()
            .to_string();
        let reference: OutputReference = {
            let requests = server.requests.lock().unwrap();
            let catalog = requests[0]["tools"].as_array().unwrap();
            assert!(
                catalog
                    .iter()
                    .any(|tool| tool["function"]["name"] == "read_tool_output")
            );
            assert!(
                !catalog
                    .iter()
                    .any(|tool| { tool["function"]["name"] == "read_command" })
            );
            requests
                .iter()
                .rev()
                .flat_map(|request| request["messages"].as_array().unwrap())
                .filter(|message| message["role"] == "tool")
                .find_map(|message| {
                    let text = message["content"].as_str()?;
                    let metadata: serde_json::Value =
                        serde_json::from_str(text.lines().last()?).ok()?;
                    let output: OutputReference =
                        serde_json::from_value(metadata["output"].clone()).ok()?;
                    assert_eq!(output.call, message["tool_call_id"].as_str().unwrap());
                    assert!(text.contains("Lines 1–5;"));
                    assert!(text.contains("\n2\t  trailing spaces  \r\n"));
                    assert!(text.contains("\n4\t\t中文🙂\r\n"));
                    assert!(text.contains("\n5\tLast marker [no trailing newline]"));
                    assert_ne!(text, original);
                    Some(output)
                })
                .expect("numbered file preview and original output reference")
        };
        assert!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| {
                    matches!(part, Part::ToolCall { id: Some(id), name, .. }
                if id == &reference.call && name == &file)
                })
        );
        assert_eq!(
            reference.revision,
            blake3::hash(original.as_bytes()).to_hex().to_string()
        );
        server.replace_tools(vec![
            (
                "read_tool_output".into(),
                json!({"output":reference,"offset":0,"limit":257}),
            ),
            (
                "read_tool_output".into(),
                json!({"output":reference,"offset":257,"limit":32768}),
            ),
        ]);
        submit(
            &fixture.client,
            &fixture.session,
            "Read the earlier original result in two pages".into(),
        )
        .await;
        let page = history(&fixture.client, fixture.session.id).await;
        let pages = tool_details(&fixture.client, &page)
            .await
            .into_iter()
            .filter(|(name, _)| name == "read_tool_output")
            .map(|(_, detail)| {
                serde_json::from_value::<OutputPage>(detail.result.unwrap()).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].next_offset, Some(257));
        assert!(pages[1].next_offset.is_none());
        for page in &pages {
            assert_eq!(page.turn, source);
            assert_eq!(page.output, reference);
            assert_eq!(page.total_bytes, original.len());
        }
        assert_eq!(format!("{}{}", pages[0].text, pages[1].text), original);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("source.txt")).unwrap(),
            text
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
