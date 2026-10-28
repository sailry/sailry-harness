use super::*;
use serde_json::{Value, json};

#[path = "prompt/guides.rs"]
mod guides;
#[path = "prompt/personal.rs"]
mod personal;

const BASE: &str = include_str!("../../../../src/agent/prompt.txt");
const ROOT_RULE: &str = "Root fixture rule: preserve the requested project scope";
const NESTED_RULE: &str = "Nested fixture rule: retain the file's Unicode content";
const FIRST: &str = "Read the requested fixture file without changing it";
const SECOND: &str = "Continue the same task after restoring the session";
const PERSONAL_FIRST: &str = "Personal fixture preference: preserve concise answers";
const PERSONAL_NEXT: &str = "Updated personal fixture preference: explain verified outcomes";
const CONTROLLER_RULE: &str = "Controller preferences must not become execution instructions";

fn prepare(fixture: &Fixture) {
    std::fs::write(fixture.root.join("CLAUDE.md"), ROOT_RULE).unwrap();
    std::fs::create_dir(fixture.root.join("nested")).unwrap();
    std::fs::write(fixture.root.join("nested/GEMINI.md"), NESTED_RULE).unwrap();
    std::fs::write(
        fixture.node.profile().join("instructions.md"),
        PERSONAL_FIRST,
    )
    .unwrap();
    std::fs::write(
        fixture.directory.path().join("controller/instructions.md"),
        CONTROLLER_RULE,
    )
    .unwrap();
    for path in ["native.txt", "nested/native.txt"] {
        std::fs::write(fixture.root.join(path), "工具内容 🙂").unwrap();
    }
}

fn referenced(task: &str, directory: bool) -> Input {
    Input {
        text: task.into(),
        attachments: vec![],
        references: vec![reference::Reference {
            label: "Nested fixture".into(),
            target: if directory {
                reference::Target::Directory("nested".into())
            } else {
                reference::Target::File("nested/native.txt".into())
            },
        }],
    }
}

async fn resume(fixture: Fixture, first: Input, second: Input, remote: bool) {
    let Output::QueuedTurn(turn) = execute(
        &fixture.client,
        Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: first,
        },
    )
    .await
    else {
        panic!("turn expected")
    };
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{page:?}"
    );
    assert!(
        page.entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .any(|part| matches!(part, Part::ToolResult { .. }))
    );
    let Output::QueuedTurn(queued) = execute(
        &fixture.client,
        Command::QueueTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: second,
        },
    )
    .await
    else {
        panic!("turn expected")
    };
    std::fs::write(
        fixture.node.profile().join("instructions.md"),
        PERSONAL_NEXT,
    )
    .unwrap();
    let profile = fixture.node.profile().to_owned();
    fixture.node.shutdown().await.unwrap();
    let node = Node::start_empty(profile).await.unwrap();
    let client = Client::new(if remote {
        fixture.controller.handle().remote(node.link().address())
    } else {
        node.local()
    });
    execute(&client, Command::StartQueuedTurn { turn: queued.id }).await;
    let queue = history(&client, fixture.session.id).await.queue;
    assert!(queue.paused);
    execute(
        &client,
        Command::SetQueuePaused {
            session: fixture.session.id,
            expected_revision: queue.revision,
            paused: false,
        },
    )
    .await;
    let page = finished(&client, fixture.session.id, queued.id).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{page:?}"
    );
    assert!(page.entries.iter().all(|entry| entry.parts.iter().all(|part| {
        !matches!(part, Part::Text(text) if text.contains("Node context") || text.contains(ROOT_RULE) || text.contains(PERSONAL_FIRST))
    })));
    fixture.controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

fn content(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.into();
    }
    value
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn instruction(api: ModelApi, body: &Value) -> String {
    match api {
        ModelApi::Anthropic => content(&body["system"]),
        ModelApi::Gemini => content(&body["systemInstruction"]["parts"]),
        ModelApi::Responses | ModelApi::ChatCompletions | ModelApi::DeepSeek => {
            let messages = if api == ModelApi::Responses {
                &body["input"]
            } else {
                &body["messages"]
            };
            messages
                .as_array()
                .unwrap()
                .iter()
                .filter(|message| matches!(message["role"].as_str(), Some("system" | "developer")))
                .map(|message| content(&message["content"]))
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => unreachable!(),
    }
}

fn users(api: ModelApi, body: &Value) -> Vec<String> {
    let (messages, parts) = match api {
        ModelApi::Gemini => (&body["contents"], "parts"),
        ModelApi::Responses => (&body["input"], "content"),
        _ => (&body["messages"], "content"),
    };
    messages
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "user")
        .map(|message| content(&message[parts]))
        .collect()
}

fn assert_policy(api: ModelApi, body: &Value, resumed: bool, nested: bool) {
    let system = instruction(api, body);
    assert_eq!(system.matches(BASE.trim()).count(), 1, "{api:?}");
    for dynamic in [
        ROOT_RULE,
        NESTED_RULE,
        PERSONAL_FIRST,
        PERSONAL_NEXT,
        CONTROLLER_RULE,
        "This turn uses Ask permission",
        "Work mode\n",
        "Node environment",
    ] {
        assert!(!system.contains(dynamic), "{api:?}: {dynamic}");
    }
    let history = body.to_string();
    for (context, expected) in [
        (ROOT_RULE, 1),
        (NESTED_RULE, usize::from(nested)),
        (PERSONAL_FIRST, 1),
        (PERSONAL_NEXT, usize::from(resumed)),
        (CONTROLLER_RULE, 0),
    ] {
        assert_eq!(
            history.matches(context).count(),
            expected,
            "{api:?}: {context}"
        );
    }
    let users = users(api, body);
    assert!(
        users.iter().all(|text| !text.contains(BASE.trim())),
        "{api:?}"
    );
    assert!(
        users.iter().all(|text| !text.contains(CONTROLLER_RULE)),
        "{api:?}"
    );
    for (task, expected) in [(FIRST, 1), (SECOND, usize::from(resumed))] {
        assert!(!system.contains(task), "{api:?}: user task became policy");
        assert_eq!(
            users
                .iter()
                .map(|text| text.matches(task).count())
                .sum::<usize>(),
            expected,
            "{api:?}: {task}"
        );
    }
}

#[tokio::test]
async fn preserves_native_roles_across_resume() {
    for api in [ModelApi::Anthropic, ModelApi::Gemini, ModelApi::DeepSeek] {
        for remote in [false, true] {
            let server = Server::start(api, Reply::Tool).await;
            let fixture = Fixture::packages(remote, api, &server.endpoint, &["files"]).await;
            prepare(&fixture);
            resume(
                fixture,
                referenced(FIRST, remote),
                referenced(SECOND, remote),
                remote,
            )
            .await;
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 3, "{api:?}, remote={remote}");
            for (index, request) in requests.iter().enumerate() {
                assert_policy(api, &request.body, index == 2, true);
                assert_eq!(
                    instruction(api, &request.body),
                    instruction(api, &requests[0].body),
                    "{api:?}, remote={remote}, request={index}"
                );
                assert_eq!(
                    request.body["tools"], requests[0].body["tools"],
                    "{api:?}, remote={remote}, request={index}"
                );
            }
            assert!(requests[1].body.to_string().contains("工具内容 🙂"));
            assert!(requests[2].body.to_string().contains(server::ANSWER));
        }
    }
}

#[tokio::test]
async fn loads_rules_at_openai_tool_boundary() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        for remote in [false, true] {
            let name = crate::agent_support::plugin_tool("files", "read_file");
            let arguments = json!({"path":"nested/native.txt"});
            let server = if api == ModelApi::Responses {
                crate::agent_support::Server::phases_with_tool(&name, arguments).await
            } else {
                crate::agent_support::Server::tools(vec![(name, arguments)]).await
            };
            let fixture = Fixture::packages(remote, api, &server.endpoint, &["files"]).await;
            prepare(&fixture);
            resume(fixture, FIRST.into(), referenced(SECOND, false), remote).await;
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 3, "{api:?}, remote={remote}");
            for (index, request) in requests.iter().enumerate() {
                assert_policy(api, request, index == 2, index > 0);
                assert_eq!(instruction(api, request), instruction(api, &requests[0]));
                assert_eq!(request["tools"], requests[0]["tools"]);
            }
            assert!(requests[1].to_string().contains("工具内容 🙂"));
        }
    }
}
