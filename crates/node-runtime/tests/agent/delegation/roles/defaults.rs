use super::*;
use crate::delegation::fixture::Fixture;
use sailry_protocol::plugin::{self, conversation::Binding};

const PACKAGE: &str = "delegation-assistant";
const INSTRUCTIONS: &str = "Keep the workspace assistant's founding instructions";
const REPLACEMENT: &str = "Use the replacement assistant's instructions";
const GUIDE: &str = "Preserve the project instruction fixture";
const INITIAL: &str = "Summarize the independent child task";
const FOLLOWUP: &str = "Continue the same independent child task";
const PERSONAL_FIRST: &str = "Initial personal preference for parent and child";
const PERSONAL_NEXT: &str = "Updated personal preference for the next runner";
const CONTROLLER_RULE: &str = "Unrelated controller preference";

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

async fn install(fixture: &Fixture, revision: u64, context: &str) -> plugin::Info {
    let root = fixture.root().join("assistant");
    std::fs::create_dir_all(root.join(plugin::NAMESPACE)).unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/main.js"),
        "import { View, div } from 'gpui-kit'; export default class Assistant extends View { render() { return div(); } }",
    )
    .unwrap();
    std::fs::write(
        root.join("plugin.json"),
        json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name":PACKAGE,
            "version":if revision == 0 { "0.1.0" } else { "0.2.0" },
            "extensions":{"dev.sailry.platform":{
                "api_version":"v1",
                "scope":"desktop",
                "desktop":{
                    "entry":"dev.sailry.platform/main.js",
                    "resources":["dev.sailry.platform/main.js"],
                    "conversations":[{
                        "id":"workspace",
                        "resource":"workspace",
                        "context":context,
                        "tools":[
                            {"kind":"package","package":"delegation","name":"spawn_agent"},
                            {"kind":"package","package":"files","name":"read_file"}
                        ]
                    }]
                }
            }}
        })
        .to_string(),
    )
    .unwrap();
    let Output::Plugin(info) = execute(
        &fixture.client,
        Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "assistant".into(),
            name: PACKAGE.into(),
            expected_revision: revision,
            expected_digest: None,
        },
    )
    .await
    else {
        panic!("assistant package expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    info
}

fn has_delegate(request: &Value) -> bool {
    request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["function"]["name"] == plugin_tool("delegation", "spawn_agent"))
}

fn context(request: &Value, personal: &str) -> String {
    assert_eq!(request["messages"][0]["role"], "system");
    let instructions = request["messages"][0]["content"].to_string();
    assert!(instructions.contains("You are Sailry"));
    assert!(instructions.contains(INSTRUCTIONS));
    assert!(instructions.contains(GUIDE));
    assert_eq!(instructions.matches(personal).count(), 1);
    assert!(!instructions.contains(CONTROLLER_RULE));
    let previous = if personal == PERSONAL_FIRST {
        PERSONAL_NEXT
    } else {
        PERSONAL_FIRST
    };
    assert!(!instructions.contains(previous));
    assert!(
        request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "user")
            .all(|message| {
                let content = message["content"].to_string();
                !content.contains(PERSONAL_FIRST)
                    && !content.contains(PERSONAL_NEXT)
                    && !content.contains(CONTROLLER_RULE)
            })
    );
    assert!(!instructions.contains(REPLACEMENT));
    assert!(!instructions.contains("Frozen review instructions"));
    if has_delegate(request) {
        let tool = request["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["function"]["name"] == plugin_tool("delegation", "spawn_agent"))
            .unwrap();
        let purpose = tool["function"]["parameters"]["properties"]["role"]["description"]
            .as_str()
            .unwrap();
        assert!(purpose.contains("review (Review): Inspect the delegated task"));
        assert!(purpose.contains("selection metadata only"));
        assert!(!purpose.contains("Frozen review instructions"));
    }
    request["messages"].to_string()
}

#[tokio::test]
async fn inherits_assistant_and_reuses_frozen_context() {
    for remote in [false, true] {
        let parent = Server::parallel(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"task":INITIAL}),
        )])
        .await;
        let unused = Server::start(false).await;
        let mut fixture = Fixture::start(remote, parent, unused).await;
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
        std::fs::write(fixture.root().join("AGENTS.md"), GUIDE).unwrap();
        let assistant = install(&fixture, 0, INSTRUCTIONS).await;
        let mut config = fixture.session.config.clone();
        config.assistant = Some(Binding {
            package: assistant.summary.reference(),
            id: "workspace".into(),
        });
        let Output::Session(session) = execute(
            &fixture.client,
            Command::CreateSession {
                project: fixture.session.project,
                worktree: Some(fixture.session.worktree),
                config: Some(config),
            },
        )
        .await
        else {
            panic!("assistant session expected")
        };
        let Output::Session(session) = execute(
            &fixture.client,
            Command::SetSessionRoles {
                session: session.id,
                expected_revision: session.revision,
                roles: vec![fixture.role.reference()],
            },
        )
        .await
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let Output::QueuedTurn(first) = execute(
            &fixture.client,
            Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "First parent-only private context".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        let mut provider = fixture
            .snapshot()
            .await
            .providers
            .into_iter()
            .find(|provider| provider.id == first.config.provider)
            .unwrap();
        let revision = provider.revision;
        provider.endpoint = "http://127.0.0.1:1/v1".into();
        execute(
            &fixture.client,
            Command::PutProvider {
                provider,
                expected_revision: revision,
            },
        )
        .await;
        execute(&fixture.client, Command::StartQueuedTurn { turn: first.id }).await;
        let parent_page = finished(&fixture.client, fixture.session.id, first.id).await;
        assert_eq!(parent_page.runs[0].status, Status::Completed);
        let output = results(&parent_page);
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["status"], "completed", "{output:?}");
        assert_eq!(output[0]["response"], "answer-fixture-a");
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        assert_eq!(child.config, first.config);
        assert!(child.roles.profiles.is_empty());
        assert_eq!(child.delegation.as_ref().unwrap().role, None);
        assert_eq!(
            child.profile.as_ref().unwrap().provider.endpoint,
            fixture.parent.endpoint
        );
        let original = history(&fixture.client, child.id).await;
        assert_eq!(original.runs.len(), 1);
        assert_eq!(original.runs[0].status, Status::Completed);
        let requests = fixture.parent.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        context(&requests[0], PERSONAL_FIRST);
        let request = requests
            .iter()
            .find(|request| !has_delegate(request))
            .unwrap();
        assert_eq!(request["model"], "fixture-a");
        assert_eq!(request["reasoning_effort"], "high");
        let initial = context(request, PERSONAL_FIRST);
        assert!(initial.contains(INITIAL));
        assert!(!initial.contains("parent-only private context"));
        assert!(fixture.child.requests.lock().unwrap().is_empty());

        let replacement = install(&fixture, assistant.summary.revision, REPLACEMENT).await;
        assert_ne!(replacement.summary.digest, assistant.summary.digest);
        let next_parent = Server::turn_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"session":child.id,"task":FOLLOWUP}),
        )])
        .await;
        let Output::Provider(provider) = execute(
            &fixture.client,
            Command::PutProvider {
                provider: Provider {
                    request_max_retries: None,
                    stream_max_retries: None,
                    id: ProviderId::new(),
                    revision: 0,
                    name: "Replacement parent provider".into(),
                    endpoint: next_parent.endpoint.clone(),
                    models: vec![model("fixture-new-parent")],
                    default_model: "fixture-new-parent".into(),
                    ..fixture.provider.clone()
                },
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let mut config = fixture.session.config.clone();
        config.provider = provider.id;
        config.model = "fixture-new-parent".into();
        config.effort = Effort::Low;
        let Output::Session(session) = execute(
            &fixture.client,
            Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        fixture.session = session;
        std::fs::write(
            fixture.node.profile().join("instructions.md"),
            PERSONAL_NEXT,
        )
        .unwrap();
        let second = fixture.push("Second parent-only private context").await;
        let parent_page = finished(&fixture.client, fixture.session.id, second.id).await;
        assert!(
            parent_page
                .runs
                .iter()
                .all(|run| run.status == Status::Completed)
        );
        let output = results(&parent_page);
        assert_eq!(output.len(), 2);
        assert_eq!(output[1]["session"], json!(child.id));
        assert_eq!(output[1]["status"], "completed", "{output:?}");
        assert_eq!(output[1]["response"], "answer-fixture-a");
        let retained = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        assert_eq!(retained.config, child.config);
        assert_eq!(retained.roles, child.roles);
        assert_eq!(retained.profile, child.profile);
        assert_eq!(retained.delegation, child.delegation);
        assert_eq!(retained.revision, child.revision);
        assert_eq!(retained.worktree, child.worktree);
        let page = history(&fixture.client, child.id).await;
        assert_eq!(page.runs.len(), 2);
        assert!(page.entries.starts_with(&original.entries));
        let requests = fixture.parent.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 4);
        let child_requests: Vec<_> = requests
            .iter()
            .filter(|request| !has_delegate(request))
            .collect();
        assert_eq!(child_requests.len(), 2);
        let resumed = child_requests[1];
        assert_eq!(resumed["model"], "fixture-a");
        assert_eq!(resumed["reasoning_effort"], "high");
        let inherited = context(resumed, PERSONAL_NEXT);
        assert!(inherited.contains(INITIAL));
        assert!(inherited.contains("answer-fixture-a"));
        assert!(inherited.contains(FOLLOWUP));
        assert!(!inherited.contains("parent-only private context"));
        assert!(fixture.child.requests.lock().unwrap().is_empty());
        let requests = next_parent.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["model"], "fixture-new-parent");
        assert_eq!(requests[0]["reasoning_effort"], "low");
        context(&requests[0], PERSONAL_NEXT);
        fixture.close().await;
    }
}
