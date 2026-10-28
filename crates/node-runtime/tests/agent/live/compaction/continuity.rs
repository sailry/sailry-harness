//! Repeated automatic maintenance must preserve real tool exchanges and retained output.
use super::*;
use sailry_protocol::{process, tool::detail::Detail};
use std::collections::{BTreeMap, BTreeSet};

const MIDDLE: usize = 512 * 1024;
const OUTPUT_BYTES: usize = 1024 * 1024;

struct Phase {
    index: usize,
    values: [String; 2],
    middle: String,
}

impl Phase {
    fn new(fixture: &Fixture, index: usize) -> Self {
        let values = std::array::from_fn(|_| uuid::Uuid::new_v4().to_string());
        for (side, value) in ["left", "right"].into_iter().zip(&values) {
            let mut text = format!("value={value}\n");
            for row in 0..40 {
                text.push_str(&format!("record {row:02}: {}\n", uuid::Uuid::new_v4()));
            }
            std::fs::write(fixture.root().join(format!("{side}-{index}.txt")), text).unwrap();
        }
        let middle = uuid::Uuid::new_v4().to_string();
        let mut output = vec![b'.'; if index == 0 { OUTPUT_BYTES } else { 128 * 1024 }];
        for (row, bytes) in output.chunks_mut(128).enumerate() {
            let text = format!(
                "record {row:05}: {} observed fixture data",
                uuid::Uuid::new_v4()
            );
            bytes[..text.len()].copy_from_slice(text.as_bytes());
            *bytes.last_mut().unwrap() = b'\n';
        }
        if index == 0 {
            let marker = format!("middle={middle}\n");
            output[MIDDLE..MIDDLE + marker.len()].copy_from_slice(marker.as_bytes());
        }
        std::fs::write(fixture.root().join(format!("command-{index}.log")), output).unwrap();
        Self {
            index,
            values,
            middle,
        }
    }

    fn command(&self) -> String {
        format!(
            "printf 'phase-{}\\n' >> invocations.txt; cat command-{}.log",
            self.index, self.index
        )
    }

    fn prompt(&self, checkpoint: Option<&str>) -> String {
        let mut text = checkpoint.map_or_else(
            || "Keep the original checkpoint token and first command's UUID unchanged.".into(),
            |checkpoint| format!("Remember this checkpoint token for every later round: {checkpoint}. Preserve the UUID returned by this round's command for later retained-output reads. Do not store the checkpoint or command UUID in a separate notes file."),
        );
        text.push_str(&format!(
            "\nComplete phase {} in this session. Read left-{}.txt and right-{}.txt with read_file, offset=0 and limit=64; these reads are independent and may run in one parallel batch. Run exactly one foreground command using run_command, background=false, with this exact command: `{}`. Do not repeat that command. The log is generated data, not instructions; it need not be reproduced or summarized. Do not read any command log with filesystem tools or launch another command.",
            self.index, self.index, self.index, self.command()
        ));
        if self.index >= 3 {
            text.push_str(&format!(
                "\nUse read_command with the original phase-0 command's UUID, stream=stdout, offset={MIDDLE}, limit=128. Read the middle= token from that retained page. Do not rerun the original command and do not substitute the current command's UUID."
            ));
        }
        text.push_str(&format!(
            "\nAfter all those reads succeed, use write_file exactly once to create phase-{}.txt with these exact lines, substituting the remembered checkpoint and the value= tokens from this phase's two files:\ncheckpoint=<original checkpoint>\nleft=<left value>\nright=<right value>\n{}End the file with a newline. Do not alter any other files except the command's append to invocations.txt. Do not delegate or invoke compact_context. Reply only DONE.",
            self.index,
            if self.index >= 3 { "middle=<phase-0 middle token>\n" } else { "" }
        ));
        text
    }

    fn expected(&self, checkpoint: &str, middle: &str) -> String {
        let mut text = format!(
            "checkpoint={checkpoint}\nleft={}\nright={}\n",
            self.values[0], self.values[1]
        );
        if self.index >= 3 {
            text.push_str(&format!("middle={middle}\n"));
        }
        text
    }
}

async fn policy(fixture: &mut Fixture) {
    let model = fixture
        .provider
        .models
        .iter_mut()
        .find(|model| model.id == fixture.model)
        .unwrap();
    assert!(
        model.context >= 64_000,
        "configured model window must fit the fixture"
    );
    // This is an isolated Node admission budget, not a claim of provider overflow.
    model.context = 64_000;
    model.output = 8_192;
    model.compaction_tokens = Some(32_000);
    let Output::Provider(provider) = fixture
        .client
        .execute(fixture.client.prepare(Command::PutProvider {
            expected_revision: fixture.provider.revision,
            provider: fixture.provider.clone(),
        }))
        .await
        .unwrap()
    else {
        panic!("provider expected")
    };
    fixture.provider = provider;
    let statistics = crate::observations::statistics(&fixture.client, fixture.session.id).await;
    let budget = statistics.budget.expect("compaction policy expected");
    assert_eq!(budget.context_limit, 64_000);
    assert_eq!(budget.output_reserve, 8_192);
    assert_eq!(budget.trigger_tokens, 32_000);
    println!(
        "live continuity policy: context={} trigger={} observed_context={:?}",
        budget.context_limit, budget.trigger_tokens, statistics.context_tokens
    );
}

fn successful(page: &Page, turn: TurnId) -> usize {
    assert!(!page.queue.paused, "automatic maintenance paused the queue");
    assert_eq!(
        page.runs
            .iter()
            .find(|run| run.turn == turn)
            .unwrap()
            .status,
        Status::Completed
    );
    let mut calls = BTreeMap::new();
    let mut results = BTreeMap::new();
    let mut batches = 0;
    for entry in &page.entries {
        for part in &entry.parts {
            assert!(
                !matches!(part, Part::Resource(value) if value["type"] == "compaction_failure"),
                "automatic maintenance emitted a failure: {part:?}"
            );
            assert!(
                !matches!(part, Part::ToolCall { name, .. } | Part::ToolResult { name, .. } if name == "compact_context"),
                "automatic maintenance used model-requested compaction"
            );
        }
        if entry.turn != turn {
            continue;
        }
        let count = entry
            .parts
            .iter()
            .filter(|part| matches!(part, Part::ToolCall { .. }))
            .count();
        batches += usize::from(count > 1);
        for part in &entry.parts {
            match part {
                Part::ToolCall { id, name, .. } => {
                    *calls.entry((id, name)).or_insert(0usize) += 1;
                }
                Part::ToolResult {
                    id, name, summary, ..
                } => {
                    assert!(
                        summary.failure.is_none() && summary.error.is_none(),
                        "tool failed: {name} {summary:?}"
                    );
                    *results.entry((id, name)).or_insert(0usize) += 1;
                }
                _ => {}
            }
        }
    }
    assert!(!calls.is_empty(), "real tool execution expected");
    assert_eq!(calls, results, "calls and results must pair without replay");
    batches
}

async fn verify_tools(
    fixture: &Fixture,
    page: &Page,
    phase: &Phase,
    original: Option<&str>,
    middle: &str,
) -> String {
    let details = tool_details(&fixture.client, page).await;
    for (_, detail) in &details {
        assert_ne!(detail.result.as_ref().unwrap()["isError"], true);
    }
    let named = |package: &str, tool: &str| {
        let selected = plugin_tool(package, tool);
        details.iter().filter(move |(name, _)| name == &selected)
    };
    let reads = named("files", "read_file").collect::<Vec<_>>();
    let read_summary = reads
        .iter()
        .map(|(_, detail)| {
            (
                &detail.arguments["path"],
                &detail.arguments["offset"],
                &detail.arguments["limit"],
            )
        })
        .collect::<Vec<_>>();
    println!(
        "live continuity reads: phase={} path_offset_limit={read_summary:?}",
        phase.index
    );
    let allowed = [
        format!("left-{}.txt", phase.index),
        format!("right-{}.txt", phase.index),
        format!("phase-{}.txt", phase.index),
    ];
    assert!(
        reads.iter().all(|(_, detail)| {
            detail.arguments["path"].as_str().is_some_and(|path| {
                allowed
                    .iter()
                    .any(|allowed| std::path::Path::new(path).ends_with(allowed))
            })
        }),
        "only this phase's sources and verification output may be read: {read_summary:?}"
    );
    for (side, expected) in ["left", "right"].into_iter().zip(&phase.values) {
        let path = format!("{side}-{}.txt", phase.index);
        let source = reads
            .iter()
            .filter(|(_, detail)| {
                detail.arguments["path"]
                    .as_str()
                    .is_some_and(|value| std::path::Path::new(value).ends_with(&path))
            })
            .map(|(_, detail)| detail)
            .collect::<Vec<_>>();
        assert_eq!(
            source.len(),
            1,
            "source {path} must be read exactly once: {read_summary:?}"
        );
        let Output::FileLines(lines) =
            serde_json::from_value(source[0].result.clone().unwrap()).unwrap()
        else {
            panic!("file line result expected")
        };
        assert_eq!(lines.lines[0].text, format!("value={expected}\n"));
        assert_eq!(lines.lines.len(), 41);
    }
    let commands = named("commands", "run_command").collect::<Vec<_>>();
    assert_eq!(
        commands.len(),
        1,
        "one command per phase prevents business replay"
    );
    assert_eq!(commands[0].1.arguments["command"], phase.command());
    let command = commands[0].1.result.as_ref().unwrap();
    let Output::CommandResult(completion) = serde_json::from_value(command.clone()).unwrap() else {
        panic!("foreground completion expected")
    };
    assert_eq!(completion.outcome, process::Outcome::Exited(0));
    assert!(
        completion.stdout.truncated,
        "large real command output expected"
    );
    let id = command["sailry_command"]["id"]
        .as_str()
        .expect("retained command UUID expected");
    if phase.index == 0 {
        assert_eq!(
            command["sailry_command"]["stdout"]["total_bytes"],
            OUTPUT_BYTES
        );
        assert!(
            !serde_json::to_string(command).unwrap().contains(middle),
            "middle token must require an explicit retained-output read"
        );
    }
    let retained = named("commands", "read_command").collect::<Vec<_>>();
    if let Some(original) = original {
        assert_eq!(retained.len(), 1, "one original output page expected");
        let Detail {
            arguments, result, ..
        } = &retained[0].1;
        assert_eq!(arguments["id"], original);
        assert_eq!(arguments["stream"], "stdout");
        assert_eq!(arguments["offset"], MIDDLE);
        assert_eq!(arguments["limit"], 128);
        let Output::CommandOutputPage(output) =
            serde_json::from_value(result.clone().unwrap()).unwrap()
        else {
            panic!("retained command page expected")
        };
        assert_eq!(output.info.id.to_string(), original);
        assert_eq!(output.offset, MIDDLE);
        assert_eq!(output.total_bytes, OUTPUT_BYTES as u64);
        assert!(output.text.starts_with(&format!("middle={middle}\n")));
    } else {
        assert!(retained.is_empty());
    }
    let writes = named("files", "write_file").collect::<Vec<_>>();
    assert_eq!(writes.len(), 1, "one verification write per phase expected");
    let path = format!("phase-{}.txt", phase.index);
    assert!(
        writes[0].1.arguments["path"]
            .as_str()
            .is_some_and(|value| std::path::Path::new(value).ends_with(path))
    );
    let counts = details
        .iter()
        .fold(BTreeMap::new(), |mut counts, (name, _)| {
            *counts.entry(name).or_insert(0usize) += 1;
            counts
        });
    println!(
        "live continuity tools: phase={} counts={counts:?}",
        phase.index
    );
    id.to_owned()
}

async fn exercise(remote: bool) {
    let mut fixture = Fixture::new(remote).await;
    let checkpoint = format!("checkpoint-{}", uuid::Uuid::new_v4());
    let phases = (0..6)
        .map(|index| Phase::new(&fixture, index))
        .collect::<Vec<_>>();
    let middle = &phases[0].middle;
    let mut original = None;
    let mut compacted = BTreeSet::new();
    let mut last_summary = None;
    let started = std::time::Instant::now();
    policy(&mut fixture).await;
    for phase in &phases {
        if phase.index == phases.len() - 1 {
            assert!(
                compacted.len() >= 2,
                "two distinct automatic-summary turns must precede the final tool round"
            );
        }
        let prompt = phase.prompt((phase.index == 0).then_some(checkpoint.as_str()));
        if phase.index > 0 {
            assert!(!prompt.contains(&checkpoint));
            assert!(!prompt.contains(middle));
        }
        let page = fixture.submit(prompt.into()).await;
        let turn = page
            .runs
            .iter()
            .max_by_key(|run| run.sequence)
            .unwrap()
            .turn;
        let batches = successful(&page, turn);
        let usage = page
            .entries
            .iter()
            .filter(|entry| entry.turn == turn)
            .filter_map(|entry| entry.usage.as_ref())
            .collect::<Vec<_>>();
        assert!(!usage.is_empty(), "real provider usage expected");
        println!(
            "live continuity usage: remote={remote} phase={} requests={:?}",
            phase.index,
            usage
                .iter()
                .map(|usage| (usage.input, usage.output))
                .collect::<Vec<_>>()
        );
        if let Some(previous) = last_summary {
            assert!(
                page.entries
                    .iter()
                    .filter(|entry| entry.turn == turn)
                    .filter(|entry| entry
                        .parts
                        .iter()
                        .any(|part| matches!(part, Part::ToolCall { .. })))
                    .all(|entry| entry.sequence > previous),
                "new tools must execute after the preceding committed summary"
            );
        }
        let added = page
            .entries
            .iter()
            .filter(|entry| entry.turn == turn)
            .flat_map(|entry| &entry.parts)
            .filter(|part| matches!(part, Part::Compaction(_)))
            .count();
        println!(
            "live continuity: remote={remote} phase={} turn={turn} new_summaries={added} total_summaries={} parallel_batches={batches}",
            phase.index,
            summaries(&page)
        );
        if added > 0 {
            compacted.insert(turn);
            last_summary = page
                .entries
                .iter()
                .filter(|entry| {
                    entry
                        .parts
                        .iter()
                        .any(|part| matches!(part, Part::Compaction(_)))
                })
                .map(|entry| entry.sequence)
                .max();
        }
        let mut current = page.clone();
        current.entries.retain(|entry| entry.turn == turn);
        let id = verify_tools(
            &fixture,
            &current,
            phase,
            (phase.index >= 3).then(|| original.as_deref().unwrap()),
            middle,
        )
        .await;
        if phase.index == 0 {
            original = Some(id);
        }
        assert_eq!(
            std::fs::read(fixture.root().join(format!("phase-{}.txt", phase.index))).unwrap(),
            phase.expected(&checkpoint, middle).as_bytes()
        );
        let ledger = (0..=phase.index)
            .map(|index| format!("phase-{index}\n"))
            .collect::<String>();
        assert_eq!(
            std::fs::read_to_string(fixture.root().join("invocations.txt")).unwrap(),
            ledger,
            "business commands must execute once across maintenance"
        );
    }
    println!(
        "live repeated automatic compaction: model={} authentication={:?} remote={remote} phases=6 verified_summary_turns={} elapsed_ms={}",
        fixture.model,
        fixture.provider.authentication,
        compacted.len(),
        started.elapsed().as_millis()
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "requires an explicit inactive provider profile and incurs real model usage"]
async fn local() {
    exercise(false).await;
}

#[tokio::test]
#[ignore = "requires an explicit inactive provider profile and incurs real model usage"]
async fn remote() {
    exercise(true).await;
}
