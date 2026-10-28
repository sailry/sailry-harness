//! Billable workflow checks use real models and isolated execution Nodes.
use super::compaction::Fixture;
use super::*;

fn answer(page: &Page) -> String {
    page.entries
        .iter()
        .rev()
        .filter(|entry| entry.author != "user")
        .find_map(|entry| {
            let text = entry
                .parts
                .iter()
                .filter_map(|part| match part {
                    Part::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<String>();
            (!text.is_empty()).then_some(text)
        })
        .expect("visible model answer expected")
}

#[tokio::test]
#[ignore = "requires an explicit inactive provider profile and incurs real model usage"]
async fn reports_cache_across_unchanged_turns() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        std::fs::write(
            fixture.root().join("README.md"),
            "# Acceptance fixture\nA small Rust command-line project.\n",
        )
        .unwrap();
        std::fs::write(
            fixture.root().join("Cargo.toml"),
            "[package]\nname = \"acceptance-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::create_dir(fixture.root().join("src")).unwrap();
        std::fs::write(
            fixture.root().join("src/main.rs"),
            "fn main() { println!(\"fixture\"); }\n",
        )
        .unwrap();
        let prompts = [
            "List the files and directories in the project root. Use the filesystem tools. Reply with only their names.",
            "Read README.md and explain its purpose in one sentence.",
            "Which programming language does this project use? Inspect Cargo.toml and src/main.rs if needed. Reply with only the language name.",
        ];
        for (index, prompt) in prompts.into_iter().enumerate() {
            let page = fixture.submit(prompt.into()).await;
            let turn = page
                .runs
                .iter()
                .max_by_key(|run| run.sequence)
                .unwrap()
                .turn;
            let usage = page
                .entries
                .iter()
                .filter(|entry| entry.turn == turn)
                .filter_map(|entry| entry.usage.as_ref())
                .collect::<Vec<_>>();
            assert!(!usage.is_empty(), "real token usage expected");
            for (step, usage) in usage.iter().enumerate() {
                assert!(usage.input > 0);
                assert!(usage.cached_input <= usage.input);
                println!(
                    "live cache: remote={remote} turn={} step={step} input={} cached_input={} hit_percent={:.2}",
                    index + 1,
                    usage.input,
                    usage.cached_input,
                    usage.cached_input as f64 * 100.0 / usage.input as f64
                );
            }
            if index == 0 {
                let text = answer(&page);
                assert!(text.contains("README.md") && text.contains("Cargo.toml"));
            } else if index == 2 {
                assert!(answer(&page).to_ascii_lowercase().contains("rust"));
            }
        }
        println!("live unchanged configuration: remote={remote} turns=3 verified");
        fixture.close().await;
    }
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "requires an explicit inactive provider profile and incurs real model usage"]
async fn reads_large_command_tail() {
    use std::io::Write;

    const MARKER: &str = "FINAL-SUMMARY-MARKER";
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let path = fixture.root().join("big.log");
        let mut output = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
        for index in 1..=2_000_000 {
            writeln!(output, "line {index}").unwrap();
        }
        writeln!(output, "{MARKER}").unwrap();
        output.flush().unwrap();
        drop(output);
        let bytes = std::fs::metadata(&path).unwrap().len();
        assert!(bytes > 20_000_000);
        let page = fixture
            .submit("Run exactly one command, `cat big.log`, through the command tool with background=false. Then tell me the last line of that command's output. Use read_command if the preview is insufficient; do not read the file with a separate filesystem tool.".into())
            .await;
        assert!(answer(&page).contains(MARKER));
        let calls = tool_calls(&fixture.client, &page).await;
        let commands = calls
            .iter()
            .filter(|(name, _)| name == &plugin_tool("commands", "run_command"))
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 1, "exactly one command invocation expected");
        assert_eq!(commands[0].1.arguments["command"], "cat big.log");
        assert!(
            calls
                .iter()
                .all(|(name, _)| name != &plugin_tool("files", "read_file"))
        );
        let details = tool_details(&fixture.client, &page).await;
        let result = details
            .iter()
            .filter(|(name, _)| name == &plugin_tool("commands", "run_command"))
            .filter_map(|(_, detail)| detail.result.as_ref())
            .find(|result| {
                result["sailry_command"]["stdout"]["tail"]
                    .as_str()
                    .is_some_and(|tail| tail.contains(MARKER))
            })
            .expect("command preview must preserve the output tail");
        let Output::CommandResult(completion) = serde_json::from_value(result.clone()).unwrap()
        else {
            panic!("foreground command completion expected")
        };
        assert_eq!(
            completion.outcome,
            sailry_protocol::process::Outcome::Exited(0)
        );
        assert!(completion.stdout.truncated);
        assert!(completion.stdout.text.len() <= 64 * 1024);
        let preview = &result["sailry_command"]["stdout"];
        assert_eq!(preview["total_bytes"], bytes);
        assert!(preview["tail_offset"].as_u64().unwrap() > 0);
        println!(
            "live large output: remote={remote} bytes={bytes} preview_bytes={} marker=verified",
            result.to_string().len()
        );
        fixture.close().await;
    }
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "requires an explicit inactive provider profile and incurs real model usage"]
async fn joins_ordered_guidance_during_tools() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        std::fs::write(
            fixture.root().join("alpha.rs"),
            "fn alpha() {\n    let value = 1;\n}\n",
        )
        .unwrap();
        std::fs::write(
            fixture.root().join("beta.rs"),
            "fn beta() {\n    let value = 2;\n    println!(\"{value}\");\n}\n",
        )
        .unwrap();
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "First run exactly `sleep 8; printf GUIDANCE-READY` with the command tool and background=false. After it finishes, read alpha.rs and beta.rs separately using read_file and summarize them briefly. Do not delegate this task or modify files.".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let running = tokio::time::timeout(Duration::from_secs(120), async {
            loop {
                let page = history(&fixture.client, fixture.session.id).await;
                let run = page.runs.iter().find(|run| run.turn == turn.id).unwrap();
                assert!(matches!(run.status, Status::Queued | Status::Running));
                if page.entries.iter().flat_map(|entry| &entry.parts).any(|part| {
                    matches!(part, Part::ToolCall { name, .. } if name == &plugin_tool("commands", "run_command"))
                }) {
                    break run.clone();
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("real command invocation expected before guidance");
        let messages = [
            "Write the final summary in English and include each file's exact line count. After those summaries add the literal line GUIDANCE-ONE.",
            "After GUIDANCE-ONE add a Markdown table with columns File and Lines. After the table add the literal line GUIDANCE-TWO.",
        ];
        let mut guides = Vec::new();
        for message in messages {
            let Output::QueuedTurn(guide) = fixture
                .client
                .execute(fixture.client.prepare(Command::GuideTurn {
                    recipient: turn.id,
                    expected_revision: running.revision,
                    message: message.into(),
                }))
                .await
                .unwrap()
            else {
                panic!("guidance turn expected")
            };
            guides.push(guide.id);
        }
        let page = fixture.finish(turn.id).await;
        for guide in &guides {
            let run = page.runs.iter().find(|run| run.turn == *guide).unwrap();
            assert_eq!(run.status, Status::Continued);
            assert_eq!(run.joined, Some(turn.id));
        }
        let positions = guides
            .iter()
            .map(|turn| {
                page.entries
                    .iter()
                    .position(|entry| entry.turn == *turn && entry.author == "user")
                    .expect("joined guidance input expected")
            })
            .collect::<Vec<_>>();
        assert!(positions[0] < positions[1]);
        let calls = tool_calls(&fixture.client, &page).await;
        for path in ["alpha.rs", "beta.rs"] {
            assert!(calls.iter().any(|(name, detail)| {
                name == &plugin_tool("files", "read_file")
                    && detail.arguments["path"]
                        .as_str()
                        .is_some_and(|read| std::path::Path::new(read).ends_with(path))
            }));
        }
        let text = answer(&page);
        assert_eq!(text.matches("GUIDANCE-ONE").count(), 1);
        assert_eq!(text.matches("GUIDANCE-TWO").count(), 1);
        assert!(text.find("GUIDANCE-ONE").unwrap() < text.find("GUIDANCE-TWO").unwrap());
        assert!(
            text.lines()
                .any(|line| line.contains('|') && line.contains("alpha.rs") && line.contains('3'))
        );
        assert!(
            text.lines()
                .any(|line| line.contains('|') && line.contains("beta.rs") && line.contains('4'))
        );
        println!("live guidance: remote={remote} inputs=2 joined=true order=verified");
        fixture.close().await;
    }
}
