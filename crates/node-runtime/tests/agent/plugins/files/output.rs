use super::*;
use sailry_protocol::tool::OutputReference;

fn latest(server: &Server) -> String {
    server
        .requests
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|request| {
            request["messages"]
                .as_array()?
                .iter()
                .rev()
                .find_map(|message| {
                    (message["role"] == "tool")
                        .then(|| message["content"].as_str())
                        .flatten()
                        .filter(|text| text.starts_with("source.txt\n"))
                        .map(str::to_owned)
                })
        })
        .expect("numbered file preview expected")
}

fn metadata(text: &str) -> Value {
    serde_json::from_str(text.lines().last().unwrap()).unwrap()
}

#[tokio::test]
async fn pages_after_reload_and_formats_compaction() {
    for remote in [false, true] {
        let server = Server::scripted_tools(Vec::new()).await;
        let mut fixture = process::Fixture::files(remote, &server).await;
        let mut provider = configuration::snapshot(&fixture.client).await.providers[0].clone();
        provider.models[0].context = 4_000_000;
        provider.models[0].compaction_tokens = Some(3_000_000);
        execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await;
        let lines: Vec<_> = (1..=160)
            .map(|number| {
                let ending = if number == 160 {
                    ""
                } else if number % 2 == 0 {
                    "\r\n"
                } else {
                    "\n"
                };
                format!("row-{number:04} {}\t中文🙂  {ending}", "\\\"".repeat(180))
            })
            .collect();
        let complete = lines.concat();
        std::fs::write(fixture.root.join("source.txt"), &complete).unwrap();
        let mut offset = 0;
        let mut shown = Vec::new();
        let mut first: Option<String> = None;
        loop {
            server.replace_tools(vec![(
                name("read_file"),
                json!({"path":"source.txt","offset":offset,"limit":100}),
            )]);
            let turn = submit(&fixture).await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == turn.id)
                    .unwrap()
                    .status,
                Status::Completed
            );
            let projected = latest(&server);
            assert!(serde_json::to_string(&projected).unwrap().len() <= 32 * 1024);
            let reference: OutputReference =
                serde_json::from_value(metadata(&projected)["output"].clone()).unwrap();
            let raw = tool_details(&fixture.client, &page)
                .await
                .into_iter()
                .rfind(|(tool, _)| tool == &name("read_file"))
                .unwrap()
                .1
                .result
                .unwrap();
            assert_eq!(raw["kind"], "file_lines");
            assert_eq!(raw["data"]["offset"], offset);
            assert_eq!(raw["data"]["lines"][0]["text"], lines[offset]);
            assert_eq!(
                reference.revision,
                blake3::hash(raw.to_string().as_bytes())
                    .to_hex()
                    .to_string()
            );
            let numbers: Vec<_> = projected
                .lines()
                .filter_map(|line| line.split_once('\t')?.0.trim().parse::<usize>().ok())
                .collect();
            assert_eq!(numbers[0], offset + 1);
            for number in &numbers {
                assert!(projected.contains(lines[number - 1].as_str()));
            }
            let last = *numbers.last().unwrap();
            assert!(projected.contains(&format!("Lines {}–{last};", offset + 1)));
            shown.extend(numbers);
            if let Some(first) = &first {
                let requests = server.requests.lock().unwrap();
                assert!(
                    requests.last().unwrap()["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|message| message["role"] == "tool"
                            && message["content"].as_str() == Some(first.as_str()))
                );
            } else {
                first = Some(projected.clone());
                fixture = approvals::rules::restart(fixture, remote).await;
            }
            let Some(next) = metadata(&projected)["next_offset"].as_u64() else {
                assert_eq!(last, lines.len());
                assert!(projected.contains("[no trailing newline]"));
                break;
            };
            assert_eq!(next as usize, last);
            assert!(next as usize > offset);
            offset = next as usize;
        }
        assert_eq!(shown, (1..=lines.len()).collect::<Vec<_>>());
        server.replace_tools(Vec::new());
        let Output::QueuedTurn(turn) = execute(
            &fixture.client,
            Command::CompactContext {
                focus: None,
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
            },
        )
        .await
        else {
            panic!("compaction turn expected")
        };
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Completed
        );
        {
            let requests = server.requests.lock().unwrap();
            let summary = requests
                .iter()
                .rev()
                .find(|request| crate::agent_support::compaction::is_summary(request))
                .unwrap();
            let prompt = summary["messages"][0]["content"].as_str().unwrap();
            assert!(prompt.contains("Lines 1–"));
            assert!(prompt.contains("row-0001"));
            assert!(prompt.contains("[no trailing newline]"));
            assert!(!prompt.contains("\"lines\":["));
        }
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("source.txt")).unwrap(),
            complete
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
