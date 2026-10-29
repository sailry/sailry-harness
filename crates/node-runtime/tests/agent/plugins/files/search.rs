use super::*;

#[tokio::test]
async fn explores_without_shell_approval() {
    for remote in [false, true] {
        let server = Server::tools(vec![
            (name("search_files"), json!({"query":"needle","path":"src","mode":"content","type":"rust","context":1,"limit":2})),
            (name("search_files"), json!({"query":"needle","path":"src","mode":"content","type":"rust","context":1,"limit":3,"offset":2})),
            (name("search_files"), json!({"query":"needle","path":"src"})),
            (name("search_files"), json!({"query":"needle","path":"src","mode":"count","type":"js"})),
            (name("search_files"), json!({"query":"first.*second","mode":"content","regex":true,"multiline":true,"path":"src","type":"python"})),
            (name("find_files"), json!({"globs":["**/*.rs"],"path":"src","limit":1})),
            (name("find_files"), json!({"globs":["**/*.rs"],"path":"src","limit":1,"offset":1})),
        ]).await;
        let fixture = process::Fixture::packages(remote, &server, &[]).await;
        install(&fixture).await;
        std::fs::create_dir(fixture.root.join("src")).unwrap();
        std::fs::write(fixture.root.join(".gitignore"), "src/private.rs\n").unwrap();
        for (path, content) in [
            ("main.rs", "before\nneedle\nbetween\nneedle\nafter\n"),
            ("extra.js", "needle\nneedle\n"),
            ("script.py", "first\nsecond\n"),
            ("fresh.rs", "fresh source"),
            ("private.rs", "needle private"),
        ] {
            std::fs::write(fixture.root.join("src").join(path), content).unwrap();
        }
        for (path, modified) in [("main.rs", 1000), ("fresh.rs", 2000)] {
            std::fs::File::options()
                .write(true)
                .open(fixture.root.join("src").join(path))
                .unwrap()
                .set_times(
                    std::fs::FileTimes::new()
                        .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(modified)),
                )
                .unwrap();
        }
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        assert!(page.approvals.is_empty(), "{page:?}");
        let outputs = results(&fixture.client, &page).await;
        assert_eq!(outputs.len(), 7, "{outputs:?}");
        assert_eq!(outputs[0]["data"]["next_offset"], 2);
        assert_eq!(outputs[0]["data"]["matches"][0]["matched"], false);
        assert_eq!(outputs[0]["data"]["matches"][1]["matched"], true);
        assert_eq!(outputs[1]["data"]["matches"][0]["line_number"], 3);
        assert_eq!(outputs[1]["data"]["matches"].as_array().unwrap().len(), 3);
        assert!(outputs[1]["data"]["next_offset"].is_null());
        assert_eq!(
            outputs[2]["data"]["paths"],
            json!(["src/extra.js", "src/main.rs"])
        );
        assert!(outputs[2]["data"].get("matches").is_none());
        assert_eq!(
            outputs[3]["data"]["counts"],
            json!([{"path":"src/extra.js","count":2}])
        );
        assert!(outputs[3]["data"].get("paths").is_none());
        assert_eq!(outputs[4]["data"]["matches"].as_array().unwrap().len(), 2);
        assert_eq!(outputs[5]["data"]["paths"], json!(["src/fresh.rs"]));
        assert_eq!(outputs[5]["data"]["next_offset"], 1);
        assert_eq!(outputs[6]["data"]["paths"], json!(["src/main.rs"]));
        assert!(outputs[6]["data"]["next_offset"].is_null());
        assert!(
            !serde_json::to_string(&outputs)
                .unwrap()
                .contains("needle private")
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

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
                        .filter(|text| text.starts_with("Search content;"))
                        .map(str::to_owned)
                })
        })
        .expect("compact search text expected")
}

async fn route(fixture: &process::Fixture, endpoint: &str) {
    let mut provider = configuration::snapshot(&fixture.client).await.providers[0].clone();
    provider.endpoint = endpoint.into();
    execute(
        &fixture.client,
        Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        },
    )
    .await;
}

#[tokio::test]
async fn pages_raw_results_across_reload_and_compaction() {
    use sailry_protocol::tool::{MAX_OUTPUT_PAGE_BYTES, OutputReference};
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
        let rows: Vec<_> = (1..=60)
            .map(|number| format!("needle-{number:04} {} 中文🙂  ", "\\\"".repeat(300)))
            .collect();
        let complete = rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                format!("{row}{}", if index == rows.len() - 1 { "" } else { "\r\n" })
            })
            .collect::<String>();
        std::fs::write(fixture.root.join("source.txt"), &complete).unwrap();
        let mut offset = 0;
        let mut shown = Vec::new();
        let mut first: Option<String> = None;
        loop {
            let release = Arc::new(tokio::sync::Notify::new());
            let current = Server::held_turn_completion(
                vec![(
                    name("search_files"),
                    json!({"query":"needle","path":"source.txt","mode":"content","offset":offset}),
                )],
                release.clone(),
            )
            .await;
            route(&fixture, &current.endpoint).await;
            let turn = submit(&fixture).await;
            current.wait_count(2).await;
            let page = history(&fixture.client, fixture.session.id).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Running);
            let projected = latest(&current);
            assert!(serde_json::to_string(&projected).unwrap().len() <= 32 * 1024);
            let metadata: Value = serde_json::from_str(projected.lines().last().unwrap()).unwrap();
            let reference: OutputReference =
                serde_json::from_value(metadata["output"].clone()).unwrap();
            let mut raw = String::new();
            let mut raw_offset = 0;
            loop {
                let Output::ToolOutput(original) = execute(
                    &fixture.client,
                    Command::ReadToolOutput {
                        turn: turn.id,
                        output: reference.clone(),
                        stream: None,
                        offset: raw_offset,
                        limit: MAX_OUTPUT_PAGE_BYTES,
                    },
                )
                .await
                else {
                    panic!("original search output expected")
                };
                assert_eq!(original.output, reference);
                assert_eq!(original.turn, turn.id);
                assert_eq!(original.offset, raw_offset);
                raw.push_str(&original.text);
                let Some(next) = original.next_offset else {
                    assert_eq!(original.total_bytes, raw.len());
                    break;
                };
                assert!(next > raw_offset);
                raw_offset = next;
            }
            assert_eq!(
                reference.revision,
                blake3::hash(raw.as_bytes()).to_hex().to_string()
            );
            let original: Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(original["kind"], "search_results");
            assert_eq!(original["data"]["offset"], offset);
            assert_eq!(original["data"]["matches"][0]["line"], rows[offset]);
            assert_eq!(
                original["data"]["matches"].as_array().unwrap().len(),
                rows.len() - offset
            );
            assert!(original["data"]["next_offset"].is_null());
            let numbers: Vec<_> = projected
                .lines()
                .filter_map(|line| {
                    line.split_once('\t')?
                        .0
                        .strip_suffix(':')?
                        .parse::<usize>()
                        .ok()
                })
                .collect();
            assert_eq!(numbers[0], offset + 1);
            for number in &numbers {
                assert!(projected.contains(&rows[number - 1]));
            }
            offset += numbers.len();
            shown.extend(numbers);
            release.notify_one();
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            if let Some(first) = &first {
                assert!(
                    current.requests.lock().unwrap().last().unwrap()["messages"]
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
            let Some(next) = metadata["next_offset"].as_u64() else {
                assert_eq!(offset, rows.len());
                break;
            };
            assert_eq!(next as usize, offset);
        }
        assert_eq!(shown, (1..=rows.len()).collect::<Vec<_>>());
        route(&fixture, &server.endpoint).await;
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
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        {
            let requests = server.requests.lock().unwrap();
            let summary = requests
                .iter()
                .rev()
                .find(|request| crate::agent_support::compaction::is_summary(request))
                .unwrap();
            let prompt = summary["messages"][0]["content"].as_str().unwrap();
            assert!(prompt.contains("Search content;"));
            assert!(prompt.contains("needle-0001"));
            assert!(!prompt.contains("\"matches\":["));
        }
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("source.txt")).unwrap(),
            complete
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
