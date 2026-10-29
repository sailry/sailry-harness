//! Offline estimates use the same tokenizer as context management, never provider usage.
use super::owners;
use crate::agent::{canonical, engine::tokens, output};
use goose_provider_types::{
    conversation::{
        Conversation, fix_conversation, merge_consecutive_messages_for_request,
        message::{Message, MessageContentBlock},
    },
    formats::{anthropic, google, openai, openai_responses},
    images::ImageFormat,
    model::ModelConfig,
};
use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
use sailry_protocol::{FileLines, TurnId};
use serde_json::{Value, json};

const FILE_MARKER: &str = "FILE_BODY_SENTINEL";
const SKILL_MARKER: &str = "SKILL_BODY_SENTINEL";
const NOTE: &str = "Offline o200k_base estimates, not provider usage. File history, skill history and retained request context are overlapping message subsets. Independently tokenized segments do not add to the serialized request count.";

pub(crate) struct Input {
    pub(crate) system: String,
    pub(crate) messages: Vec<Message>,
    pub(crate) tools: Vec<Tool>,
}

fn source(name: &str, id: &str, value: Value) -> (canonical::Record, Message) {
    let mut record = canonical::Record::new(
        "00000000-0000-0000-0000-000000000001"
            .parse::<TurnId>()
            .unwrap(),
        canonical::Kind::Message(canonical::Message {
            part_count: 1,
            content: vec![canonical::Block {
                index: 0,
                kind: canonical::BlockKind::ToolResponse(canonical::ToolResponse {
                    id: id.into(),
                    name: name.into(),
                    status: canonical::ToolStatus::Success,
                    result: value.clone().into(),
                    revision: blake3::hash(value.to_string().as_bytes())
                        .to_hex()
                        .to_string(),
                    attachments: vec![],
                    provider: None,
                    format: canonical::ToolFormat::Json,
                    meta: None,
                }),
            }],
            ..Default::default()
        }),
    );
    record.id = format!("fixture-record-{id}");
    record.created = chrono::DateTime::from_timestamp(0, 0).unwrap();
    let mut message = Message::user().with_tool_response(id, Ok(CallToolResult::structured(value)));
    message.id = Some(record.id.clone());
    (record, message)
}

fn synthetic() -> (Vec<canonical::Record>, Vec<Message>) {
    let mut records = Vec::new();
    let mut messages =
        vec![Message::user().with_text("Inspect the supplied files using this skill")];
    for page in 0..4 {
        let lines: Vec<_> = (1..=200)
            .map(|number| {
                json!({"number":number,
            "text":format!("let item_{page}_{number} = \\\"quoted\\\"; // {} 中文 🙂\n",
                if page == 0 && number == 1 { FILE_MARKER } else { "fixture body" })})
            })
            .collect();
        let body = lines
            .iter()
            .map(|line| line["text"].as_str().unwrap())
            .collect::<String>();
        let value = json!({"kind":"file_lines","data":{
            "path":format!("src/fixture-{page}.rs"),"lines":lines,"size":body.len(),
            "revision":blake3::hash(body.as_bytes()).to_hex().to_string(),"offset":0,"next_offset":null
        }});
        let id = format!("file-{page}");
        messages.push(
            Message::assistant()
                .with_tool_request(&id, Ok(CallToolRequestParams::new("read_file"))),
        );
        let (record, message) = source("read_file", &id, value);
        records.push(record);
        messages.push(message);
    }
    let content = format!(
        "{SKILL_MARKER}\n{}",
        "Check each captured input and preserve the original evidence.\n".repeat(100)
    );
    for index in 0..3 {
        let id = format!("skill-{index}");
        let name = if index == 0 {
            "load_skill"
        } else {
            "read_skill_resource"
        };
        messages.push(
            Message::assistant().with_tool_request(&id, Ok(CallToolRequestParams::new(name))),
        );
        let (record, message) = source(
            name,
            &id,
            json!({
                "skill":"fixture:review","path":"SKILL.md","content":content,
                "directory":"/fixture/package/digest/review","python":"/fixture/python"
            }),
        );
        records.push(record);
        messages.push(message);
    }
    for (index, message) in messages.iter_mut().enumerate() {
        message.created = 0;
        message
            .id
            .get_or_insert_with(|| format!("fixture-message-{index}"));
    }
    (records, messages)
}

pub(crate) fn inputs() -> (Input, Input) {
    let (records, mut messages) = synthetic();
    let original = serde_json::to_value(&records).unwrap();
    let owners = owners::collect(&records);
    let system = "Fixed fixture system instructions".to_owned();
    let mut context = super::context(
        &owners
            .parts()
            .into_iter()
            .map(|(_, part)| part)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    context.id = Some("fixture-turn-context".into());
    context.created = 0;
    messages.push(context);
    let tools: Vec<_> = ["read_file", "load_skill", "read_skill_resource"].map(|name| Tool::new(
        name,"Read captured fixture data",
        serde_json::from_value::<serde_json::Map<String, Value>>(json!({"type":"object","properties":{"path":{"type":"string"}},"additionalProperties":false})).unwrap()
    )).into();
    let mut projected = messages.clone();
    output::project(&mut projected);
    owners.project(&mut projected);
    assert_eq!(serde_json::to_value(&records).unwrap(), original);
    (
        Input {
            system: system.clone(),
            messages,
            tools: tools.clone(),
        },
        Input {
            system,
            messages: projected,
            tools,
        },
    )
}

fn payloads(input: &Input) -> [(&'static str, Value); 4] {
    let config = ModelConfig::new("fixture-input").with_max_tokens(Some(8192));
    let Input {
        system,
        messages,
        tools,
    } = input;
    [
        (
            "chat",
            openai::create_request(&config, system, messages, tools, &ImageFormat::OpenAi, true)
                .unwrap(),
        ),
        (
            "responses",
            openai_responses::create_responses_request(&config, system, messages, tools).unwrap(),
        ),
        (
            "anthropic",
            anthropic::create_request(
                "anthropic",
                &config,
                system,
                messages,
                tools,
                Default::default(),
            )
            .unwrap(),
        ),
        (
            "google",
            google::create_request(&config, system, messages, tools).unwrap(),
        ),
    ]
}

#[test]
fn keeps_context_after_tool_pairs() {
    for paired in [false, true] {
        let mut messages = vec![Message::user().with_text("INITIAL_TASK")];
        if paired {
            messages.extend([
                Message::assistant().with_tool_request(
                    "pair-call",
                    Ok(CallToolRequestParams::new("fixture_probe")),
                ),
                Message::user().with_tool_response(
                    "pair-call",
                    Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                        "PAIR_RESULT_MARKER",
                    )])),
                ),
            ]);
        }
        let tail = super::context(&["CURRENT_CONTEXT_MARKER 中文 🙂".into()]).unwrap();
        let expected = tail.as_concat_text();
        messages.push(tail);
        for (index, message) in messages.iter_mut().enumerate() {
            message.created = 0;
            message.id = Some(format!("fixture-order-{index}"));
        }
        let original = serde_json::to_value(&messages).unwrap();
        // Match the pinned inference path, including consecutive-user merging.
        let (fixed, issues) = fix_conversation(Conversation::new_unvalidated(messages.clone()));
        assert!(issues.is_empty(), "{issues:?}");
        let input = Input {
            system: "Fixed fixture system instructions".into(),
            messages: merge_consecutive_messages_for_request(fixed.messages().clone()),
            tools: Vec::new(),
        };
        if !paired {
            assert_eq!(input.messages.len(), 1);
        }
        for (name, body) in payloads(&input) {
            let field = match name {
                "responses" => "input",
                "google" => "contents",
                _ => "messages",
            };
            let sequence = body[field].to_string();
            assert_eq!(
                sequence.matches("CURRENT_CONTEXT_MARKER").count(),
                1,
                "{name}"
            );
            let last = body[field].as_array().unwrap().last().unwrap();
            assert_eq!(last["role"], "user", "{name}");
            let content = last.get("content").or_else(|| last.get("parts")).unwrap();
            let text = content
                .as_str()
                .or_else(|| content.as_array()?.last()?.get("text")?.as_str())
                .unwrap();
            assert!(text.ends_with(&expected), "{name}: {text}");
            if paired {
                let call = sequence.find("fixture_probe").unwrap();
                let result = sequence.find("PAIR_RESULT_MARKER").unwrap();
                let context = sequence.find("CURRENT_CONTEXT_MARKER").unwrap();
                assert!(call < result && result < context, "{name}: {sequence}");
            } else {
                assert!(
                    sequence.find("INITIAL_TASK").unwrap()
                        < sequence.find("CURRENT_CONTEXT_MARKER").unwrap()
                );
            }
        }
        assert_eq!(serde_json::to_value(&messages).unwrap(), original);
    }
}

pub(crate) fn counts(wire: &Value, system: &Value, messages: &Value) -> Value {
    json!({
        "system_segment":tokens::text(&system.to_string()),
        "messages_segment":tokens::text(&messages.to_string()),
        "tools_segment":tokens::text(&wire["tools"].to_string()),
        "serialized_request":tokens::text(&wire.to_string()),
        "wire_bytes":wire.to_string().len(),
    })
}

fn segments(name: &str, wire: &Value) -> Value {
    match name {
        "chat" | "responses" => {
            let key = if name == "chat" { "messages" } else { "input" };
            let (system, messages): (Vec<_>, Vec<_>) =
                wire[key].as_array().unwrap().iter().partition(|message| {
                    matches!(message["role"].as_str(), Some("system" | "developer"))
                });
            counts(wire, &json!(system), &json!(messages))
        }
        "anthropic" => counts(wire, &wire["system"], &wire["messages"]),
        "google" => counts(wire, &wire["systemInstruction"], &wire["contents"]),
        _ => unreachable!(),
    }
}

fn history_tokens(messages: &[Message], prefix: &str) -> usize {
    messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|content| {
            let response = content.as_tool_response()?;
            response
                .id
                .starts_with(prefix)
                .then(|| content.as_tool_response_text().unwrap())
        })
        .map(|text| tokens::text(&text))
        .sum()
}

fn skill_body_tokens(input: &Input) -> usize {
    input
        .messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|content| {
            let response = content.as_tool_response()?;
            response
                .id
                .starts_with("skill-")
                .then_some(response)?
                .tool_result
                .as_ref()
                .ok()?
                .structured_content
                .as_ref()?["content"]
                .as_str()
        })
        .map(tokens::text)
        .sum()
}

#[test]
fn compares_native_requests() {
    let (before, after) = inputs();
    let (repeat, _) = inputs();
    assert_eq!(
        serde_json::to_value(&before.messages).unwrap(),
        serde_json::to_value(&repeat.messages).unwrap()
    );
    let files = file_totals(&synthetic().0);
    assert_eq!(files["source_lines"], 800);
    assert_eq!(files["shown_lines"], 800);
    assert_eq!(files["unshown_lines"], 0);
    assert_eq!(files["long_line_omissions"], 0);
    assert_eq!(before.system, after.system);
    assert_eq!(
        serde_json::to_value(&before.tools).unwrap(),
        serde_json::to_value(&after.tools).unwrap()
    );
    for ((name, original), (_, projected)) in payloads(&before).into_iter().zip(payloads(&after)) {
        let original_text = original.to_string();
        let projected_text = projected.to_string();
        assert_eq!(original_text.matches(FILE_MARKER).count(), 1, "{name}");
        assert_eq!(projected_text.matches(FILE_MARKER).count(), 1, "{name}");
        assert_eq!(original_text.matches(SKILL_MARKER).count(), 4, "{name}");
        assert_eq!(projected_text.matches(SKILL_MARKER).count(), 1, "{name}");
        assert!(!projected_text.contains("\\\"lines\\\":"), "{name}");
        assert!(!projected_text.contains("structuredContent"), "{name}");
        assert!(
            tokens::text(&projected_text) < tokens::text(&original_text),
            "{name}"
        );
        println!(
            "{}",
            json!({"scope":"synthetic_fixed_catalog","protocol":name,"note":NOTE,
            "scope_note":"Fixed system instructions and three tools isolate file/skill projection; prompt instruction trimming and deferred-tool savings are not measured",
            "before":segments(name,&original),"after":segments(name,&projected),"files":files,
            "overlapping_subsets":{
                "file_body_full":files["full_plain_body_tokens"],
                "skill_body_history_before":skill_body_tokens(&before),
                "skill_body_history_after":skill_body_tokens(&after),
                "retained_skill_body":after.messages.last().unwrap().as_concat_text().lines()
                    .filter_map(|line|serde_json::from_str::<Value>(line).ok())
                    .filter_map(|value|value["content"].as_str().map(tokens::text)).sum::<usize>(),
                "file_history_before":history_tokens(&before.messages,"file-"),
                "file_history_after":history_tokens(&after.messages,"file-"),
                "skill_history_before":history_tokens(&before.messages,"skill-"),
                "skill_history_after":history_tokens(&after.messages,"skill-"),
                "retained_owner_context":tokens::text(&after.messages.last().unwrap().as_concat_text()),
            }})
        );
    }
}

/// Full numbered text isolates formatting from the production byte/line budgets.
fn unbounded(value: &Value, id: &str) -> String {
    let page: FileLines = serde_json::from_value(value["data"].clone()).unwrap();
    let mut text = if let (Some(first), Some(last)) = (page.lines.first(), page.lines.last()) {
        format!(
            "{}\nLines {}–{}; {} bytes; revision {}\n",
            page.path, first.number, last.number, page.size, page.revision
        )
    } else {
        format!(
            "{}\nNo lines at offset={}; {} bytes; revision {}\n",
            page.path, page.offset, page.size, page.revision
        )
    };
    for line in &page.lines {
        text.push_str(&format!("{}\t{}", line.number, line.text));
        if !line.text.ends_with('\n') {
            text.push_str(" [no trailing newline]\n");
        }
    }
    if let Some(next) = page.next_offset {
        text.push_str(&format!("Next file page: read_file with offset={next}\n"));
    }
    text.push_str(&json!({"output":{"call":id,"revision":blake3::hash(value.to_string().as_bytes()).to_hex().to_string()},"next_offset":page.next_offset}).to_string());
    text
}

fn tool_text(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(MessageContentBlock::as_tool_response_text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn file_totals(records: &[canonical::Record]) -> Value {
    let mut pages = 0;
    let mut source_lines = 0;
    let mut shown_lines = 0;
    let mut long_lines = 0;
    let mut raw_tokens = 0;
    let mut plain_tokens = 0;
    let mut numbered_tokens = 0;
    let mut projected_tokens = 0;
    let mut escaped_tokens = 0;
    let mut wire_tokens = 0;
    for response in records
        .iter()
        .filter_map(canonical::Record::message)
        .flat_map(|message| &message.content)
        .filter_map(|block| match &block.kind {
            canonical::BlockKind::ToolResponse(response)
                if response.result.value["kind"] == "file_lines" =>
            {
                Some(response)
            }
            _ => None,
        })
    {
        let value = &response.result.value;
        let page: FileLines = serde_json::from_value(value["data"].clone()).unwrap();
        let id = response.id.as_str();
        let mut messages = [
            Message::user().with_tool_response(id, Ok(CallToolResult::structured(value.clone())))
        ];
        output::project(&mut messages);
        let projected = tool_text(&messages[0]);
        assert!(serde_json::to_string(&projected).unwrap().len() <= 32 * 1024);
        for line in projected.lines() {
            if line.split_once('\t').is_some_and(|(number, _)| {
                number
                    .trim()
                    .parse::<usize>()
                    .is_ok_and(|number| page.lines.iter().any(|line| line.number == number))
            }) {
                shown_lines += 1;
                long_lines +=
                    usize::from(line.contains(" content bytes omitted from this line ...]"));
            }
        }
        pages += 1;
        source_lines += page.lines.len();
        raw_tokens += tokens::text(&value.to_string());
        plain_tokens += tokens::text(
            &page
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<String>(),
        );
        numbered_tokens += tokens::text(&unbounded(value, id));
        projected_tokens += tokens::text(&projected);
        escaped_tokens += tokens::text(&serde_json::to_string(&projected).unwrap());
        let payload = openai::create_request(
            &ModelConfig::new("fixture-input"),
            "",
            &messages,
            &[],
            &ImageFormat::OpenAi,
            true,
        )
        .unwrap();
        wire_tokens += tokens::text(&payload["messages"].to_string());
    }
    json!({"pages":pages,"source_lines":source_lines,"shown_lines":shown_lines,
        "unshown_lines":source_lines-shown_lines,"long_line_omissions":long_lines,
        "raw_json_tokens":raw_tokens,"full_plain_body_tokens":plain_tokens,
        "full_numbered_format_tokens":numbered_tokens,"budgeted_numbered_tokens":projected_tokens,
        "budgeted_escaped_string_tokens":escaped_tokens,"isolated_chat_messages_tokens":wire_tokens})
}

#[test]
#[ignore = "Requires explicit private records; emits only aggregate offline estimates"]
fn measures_private_records() {
    let path = std::env::var_os("SAILRY_TOKEN_RECORDS")
        .expect("set SAILRY_TOKEN_RECORDS to a private records JSONL file");
    let input = std::fs::read_to_string(path).expect("read private records");
    let records: Vec<canonical::Record> = input
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let value: Value = serde_json::from_str(line).expect("decode private record envelope");
            serde_json::from_value(value["body"].clone()).expect("decode canonical private record")
        })
        .collect();
    let mut messages = Vec::new();
    for record in &records {
        for response in record
            .message()
            .into_iter()
            .flat_map(|message| &message.content)
            .filter_map(|block| match &block.kind {
                canonical::BlockKind::ToolResponse(response)
                    if matches!(response.name.as_str(), "load_skill" | "read_skill_resource") =>
                {
                    Some(response)
                }
                _ => None,
            })
        {
            let mut message = Message::user().with_tool_response(
                response.id.as_str(),
                Ok(CallToolResult::structured(response.result.value.clone())),
            );
            message.id = Some(record.id.clone());
            messages.push(message);
        }
    }
    let before = messages
        .iter()
        .map(tool_text)
        .map(|text| tokens::text(&text))
        .sum::<usize>();
    let owners = owners::collect(&records);
    owners.project(&mut messages);
    let after = messages
        .iter()
        .map(tool_text)
        .map(|text| tokens::text(&text))
        .sum::<usize>();
    println!(
        "{}",
        json!({"scope":"private_recoverable_subsets","note":NOTE,
        "final_request":"Unavailable: captured system and tool catalog are absent; historical provider usage is unchanged",
        "files":file_totals(&records),"skills":{"results":messages.len(),"raw_history_tokens":before,
            "projected_history_tokens":after,"retained_owner_context_tokens":owners.parts().iter().filter(|(key,_)|key.starts_with("skill:")).map(|(_,text)|tokens::text(text)).sum::<usize>()}})
    );
}
