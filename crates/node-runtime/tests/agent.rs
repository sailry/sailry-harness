#[path = "agent/activity.rs"]
mod activity;
#[expect(
    dead_code,
    reason = "The shared model fixture includes response modes used by other integration test binaries"
)]
mod agent_support;
#[path = "agent/approvals.rs"]
mod approvals;
#[cfg(unix)]
#[path = "agent/artifacts.rs"]
mod artifacts;
#[path = "agent/browser.rs"]
mod browser;
#[path = "agent/checkpoints.rs"]
mod checkpoints;
#[cfg(unix)]
#[path = "agent/compaction.rs"]
mod compaction;
#[path = "agent/configuration.rs"]
mod configuration;
#[path = "agent/connections.rs"]
mod connections;
#[cfg(all(unix, feature = "test-support"))]
#[path = "support/connectors.rs"]
mod connector_fixture;
#[path = "support/database.rs"]
mod database_fixture;
#[path = "agent/delegation/mod.rs"]
mod delegation;
#[path = "agent/efficiency.rs"]
mod efficiency;
#[cfg(unix)]
#[path = "agent/environment.rs"]
mod environment;
#[cfg(unix)]
#[path = "support/environment.rs"]
mod environment_fixture;
#[path = "agent/external_browser.rs"]
mod external_browser;
#[path = "agent/forks.rs"]
mod forks;
#[cfg(all(unix, feature = "test-support"))]
#[path = "support/github.rs"]
mod github_fixture;
#[cfg(unix)]
#[path = "agent/goals.rs"]
mod goals;
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[path = "agent/hooks.rs"]
mod hooks;
#[cfg(unix)]
#[path = "agent/mcp/mod.rs"]
mod mcp;
#[path = "agent/media.rs"]
mod media;
#[cfg(unix)]
#[path = "agent/memory.rs"]
mod memory;
#[cfg(unix)]
#[path = "support/mcp_oauth.rs"]
mod oauth_server;
#[path = "agent/observations.rs"]
mod observations;
#[path = "agent/office.rs"]
mod office;
#[path = "agent/paging.rs"]
mod paging;
#[path = "agent/permissions.rs"]
mod permissions;
#[cfg(unix)]
#[path = "agent/plan_storage.rs"]
mod plan_storage;
#[cfg(unix)]
#[path = "agent/planning.rs"]
mod planning;
#[path = "agent/plugins.rs"]
mod plugins;
#[path = "agent/process.rs"]
mod process;
#[cfg(unix)]
#[path = "agent/progress.rs"]
mod progress;
#[path = "agent_support/provider.rs"]
mod provider_fixture;
#[path = "agent/providers.rs"]
mod providers;
#[path = "agent/questions.rs"]
mod questions;
#[path = "agent/queue.rs"]
mod queue;
#[path = "agent/references.rs"]
mod references;
#[path = "support/retry.rs"]
mod retry_fixture;
#[path = "agent/rewind.rs"]
mod rewind;
#[path = "agent/sandbox.rs"]
mod sandbox;
#[path = "agent/search.rs"]
mod search;
#[expect(
    dead_code,
    reason = "The shared search fixture also supports configuration-only driver scenarios"
)]
#[path = "support/search.rs"]
mod search_fixture;
#[path = "agent/skills/mod.rs"]
mod skills;
#[cfg(feature = "test-support")]
#[path = "agent/telemetry.rs"]
mod telemetry;
#[cfg(unix)]
#[path = "agent/title.rs"]
mod title;
#[path = "agent/tools.rs"]
mod tools;
#[cfg(target_os = "macos")]
#[path = "support/trash.rs"]
mod trash_fixture;
#[path = "agent/usage.rs"]
mod usage;
#[path = "agent/web.rs"]
mod web;
#[path = "support/web.rs"]
mod web_fixture;
#[path = "agent/worktrees.rs"]
mod worktrees;

use agent_support::details::{tool_calls, tool_detail, tool_details};
use agent_support::*;
use sailry_client::Client;
use sailry_link::{Link, NetworkScope, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::{conversation::*, *};
use std::{sync::Arc, time::Duration};

// Functional waits include subscription setup and delivery of the initial snapshot.
const COMPLETION_BUDGET: Duration = Duration::from_secs(30);

fn is_context(entry: &Entry) -> bool {
    entry.author == "system" && entry.parts.is_empty()
}

fn message_text(value: &serde_json::Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_owned();
    }
    value
        .as_array()
        .expect("message text or parts expected")
        .iter()
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn node_context(request: &serde_json::Value, section: &str) -> String {
    request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .filter(|message| matches!(message["role"].as_str(), Some("system" | "developer")))
        .map(|message| message_text(&message["content"]))
        .find(|text| text.contains(&format!("\n\n{section}")))
        .expect("captured Node context expected")
}

fn turn_context(request: &serde_json::Value) -> &str {
    agent_support::turn_context(request).expect("request-only turn context expected")
}

async fn planning_denial(client: &Client, page: &Page, turn: TurnId) {
    let run = page.runs.iter().find(|run| run.turn == turn).unwrap();
    assert_eq!(run.status, Status::Completed, "{run:?}");
    assert!(run.error.is_none());
    let mut page = page.clone();
    page.entries.retain(|entry| entry.turn == turn);
    assert!(tool_details(client, &page).await.iter().any(|(_, detail)| {
        detail.result.as_ref().is_some_and(|result| {
            result["isError"] == true
                && result["content"].as_array().is_some_and(|content| {
                    content.iter().any(|block| {
                        block["text"]
                            .as_str()
                            .is_some_and(|text| text.contains("planning turns"))
                    })
                })
        })
    }));
    assert!(page.approvals.iter().all(|approval| approval.turn != turn));
}

async fn isolate_model_tools(client: &Client) {
    // Feature fixtures isolate shipped external tools; their own lifecycle suites
    // explicitly enable the packages they exercise.
    disable_tools(client, &["web-search", "goals", "office"]).await;
}

async fn disable_tools(client: &Client, names: &[&str]) {
    for &name in names {
        let Output::Plugin(package) = client
            .execute(client.prepare(Command::ReadPlugin { name: name.into() }))
            .await
            .unwrap()
        else {
            panic!("shipped package expected")
        };
        if package.summary.enabled {
            client
                .execute(client.prepare(Command::SetPluginEnabled {
                    name: package.summary.name,
                    expected_revision: package.summary.revision,
                    enabled: false,
                }))
                .await
                .unwrap();
        }
    }
}

async fn configured(
    client: &Client,
    endpoint: &str,
    root: &std::path::Path,
) -> (Session, Provider) {
    // Model fixtures isolate newly shipped dependencies. Their integration
    // suites explicitly enable the packages they exercise.
    disable_tools(client, &["context7", "github", "code-review"]).await;
    configured_session(client, endpoint, root).await
}

async fn configured_session(
    client: &Client,
    endpoint: &str,
    root: &std::path::Path,
) -> (Session, Provider) {
    let provider = Provider {
        request_max_retries: None,
        stream_max_retries: None,
        preset: None,
        binding: None,
        consent: None,
        oauth: None,
        options: None,
        id: ProviderId::new(),
        revision: 0,
        name: "Model fixture".into(),
        api: ModelApi::ChatCompletions,
        authentication: sailry_protocol::Authentication::ApiKey,
        endpoint: endpoint.into(),
        enabled: true,
        models: Vec::new(),
        default_model: String::new(),
        credential: None,
    };
    let Output::Provider(provider) = client
        .execute(client.prepare(Command::PutProvider {
            provider,
            expected_revision: 0,
        }))
        .await
        .unwrap()
    else {
        panic!("provider expected")
    };
    let Output::Project(project) = client
        .execute(client.prepare(Command::RegisterProject {
            name: "Agent fixture".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    let Output::Session(session) = client
        .execute(client.prepare(Command::CreateSession {
            project: Some(project.id),
            worktree: None,
            config: Some(SessionConfig {
                features: Default::default(),
                assistant: None,
                resource: None,
                provider: provider.id,
                model: "fixture-a".into(),
                effort: Effort::High,
                mode: sailry_protocol::WorkMode::Code,
                options: Default::default(),
                permission: sailry_protocol::Permission::Ask,
                credential: None,
            }),
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    (session, provider)
}

async fn history(client: &Client, session: SessionId) -> Page {
    let Output::Conversation(page) = client
        .execute(client.prepare(Command::ReadConversation {
            session,
            before: None,
            limit: 100,
        }))
        .await
        .unwrap()
    else {
        panic!("history expected")
    };
    page.page
}

async fn results(client: &Client, page: &Page) -> Vec<serde_json::Value> {
    tool_details(client, page)
        .await
        .into_iter()
        .map(|(_, detail)| detail.result.unwrap())
        .collect()
}

async fn finished(client: &Client, session: SessionId, turn: TurnId) -> Page {
    finished_within(client, session, turn, COMPLETION_BUDGET).await
}

async fn finished_within(
    client: &Client,
    session: SessionId,
    turn: TurnId,
    budget: Duration,
) -> Page {
    let mut stage = "subscribe";
    let mut updates_seen = 0usize;
    let mut recoveries = 0usize;
    let mut last_snapshot = None;
    let mut observed = None;
    tokio::time::timeout(budget, async {
        let mut updates = client.subscribe_conversation(session).await.unwrap();
        let mut projection =
            sailry_client::conversation::Projection::new(client.target(), session, 0);
        loop {
            stage = "next update";
            let update = updates.next().await.unwrap();
            updates_seen += 1;
            if let Update::ConversationSnapshot(value) = &update {
                last_snapshot = Some((value.page.runs.len(), value.page.entries.len(), value.missing.len()));
            }
            let applied = projection.apply(0, update).unwrap();
            if applied == sailry_client::Apply::Recover {
                recoveries += 1;
                stage = "recover subscription";
                updates = client.subscribe_conversation(session).await.unwrap();
                projection =
                    sailry_client::conversation::Projection::new(client.target(), session, 0);
                continue;
            }
            if projection.snapshot().is_some_and(|snapshot| {
                let Some(run) = snapshot.page.runs.iter().find(|run| run.turn == turn) else {
                    return false;
                };
                observed = Some((run.status, run.error.clone(), snapshot.page.entries.len()));
                !matches!(run.status, Status::Queued | Status::Running | Status::Stopping)
            }) {
                // The status frame can precede other entries from its committed transaction.
                stage = "canonical history";
                let page = history(client, session).await;
                if page.runs.iter().any(|run| {
                    run.turn == turn
                        && !matches!(
                            run.status,
                            Status::Queued | Status::Running | Status::Stopping
                        )
                }) {
                    return page;
                }
            }
        }
    })
    .await
    .unwrap_or_else(|error| {
        panic!("Agent completion deadline ({budget:?}, session={session}, turn={turn}): {error:?}; stage={stage}; updates={updates_seen}; recoveries={recoveries}; last_snapshot={last_snapshot:?}; observed={observed:?}")
    })
}

fn text(page: &Page) -> String {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| {
            if let Part::Text(text) = part {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect()
}

#[tokio::test]
async fn shares_frozen_runs_after_restart() {
    for remote in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let profile = fixture.path().join("node");
        let node = Node::start_empty(&profile).await.unwrap();
        let controller =
            Link::controller(fixture.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let transport: Arc<dyn Transport> = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Client::new(transport);
        let first = Server::start(false).await;
        let second = Server::start(false).await;
        let (session, mut provider) = configured_session(&client, &first.endpoint, &root).await;
        provider.default_model = "fixture-a".into();
        provider.models = ["fixture-a", "fixture-b"]
            .into_iter()
            .map(|id| Model {
                id: id.into(),
                display_name: None,
                context: 128_000,
                output: 2048,
                vision: false,
                tools: true,
                reasoning: true,
                generates: Vec::new(),
                efforts: vec![Effort::Low, Effort::High],
                capabilities: Some(discovery::Capabilities {
                    reasoning: Some(true),
                    tools: Some(true),
                    ..Default::default()
                }),
                custom_efforts: false,
                compaction_tokens: None,
                default_effort: Effort::High,
            })
            .collect();
        let Output::Provider(mut provider) = client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let queued = client.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: 1,
            message: "first prompt".into(),
        });
        let Output::QueuedTurn(turn) = client.execute(queued.clone()).await.unwrap() else {
            panic!("turn expected")
        };
        assert_eq!(
            client.execute(queued).await.unwrap(),
            Output::QueuedTurn(turn.clone())
        );
        assert!(first.requests.lock().unwrap().is_empty());
        provider.endpoint = second.endpoint.clone();
        client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        let mut config = session.config.clone();
        config.model = "fixture-b".into();
        config.effort = Effort::Low;
        client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: 1,
                config,
            }))
            .await
            .unwrap();
        let launch = client.prepare(Command::StartQueuedTurn { turn: turn.id });
        client.execute(launch.clone()).await.unwrap();
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        assert!(
            text(&page).contains("answer-fixture-a"),
            "{:?}",
            page.entries
        );
        let requests = first.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["model"], "fixture-a");
        assert_eq!(requests[0]["reasoning_effort"], "high");
        drop(requests);
        let submit = client.prepare(Command::SubmitTurn {
            session: session.id,
            expected_revision: 2,
            message: "second prompt".into(),
        });
        let Output::QueuedTurn(next) = client.execute(submit.clone()).await.unwrap() else {
            panic!("turn expected")
        };
        controller.handle().disconnect(node.id()).await;
        let page = finished(&client, session.id, next.id).await;
        assert!(
            page.runs.iter().all(|run| run.status == Status::Completed),
            "{:?}",
            page.runs
        );
        assert!(text(&page).contains("answer-fixture-b"));
        let requests = second.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["model"], "fixture-b");
        assert_eq!(requests[0]["reasoning_effort"], "low");
        assert!(
            requests[0]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["content"]
                    .as_str()
                    .is_some_and(|text| text.contains("answer-fixture-a")))
        );
        drop(requests);
        let Output::Conversation(recent) = client
            .execute(client.prepare(Command::ReadConversation {
                session: session.id,
                before: None,
                limit: 1,
            }))
            .await
            .unwrap()
        else {
            panic!("history expected")
        };
        assert_eq!(recent.page.runs.len(), 1);
        assert_eq!(
            recent
                .page
                .entries
                .iter()
                .filter(|entry| !is_context(entry))
                .count(),
            2
        );
        assert!(recent.page.next_before.is_some());
        let Output::Conversation(earlier) = client
            .execute(client.prepare(Command::ReadConversation {
                session: session.id,
                before: recent.page.next_before,
                limit: 100,
            }))
            .await
            .unwrap()
        else {
            panic!("history expected")
        };
        assert!(
            earlier
                .page
                .entries
                .iter()
                .all(|entry| entry.sequence < recent.page.entries[0].sequence)
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        let node = Node::start_empty(&profile).await.unwrap();
        let client = Client::new(node.local());
        assert_eq!(history(&client, session.id).await, page);
        // The local caller identity remains stable; remote receipts are checked by
        // the original controller in the shared durable-request tests.
        if !remote {
            assert_eq!(
                client.execute(submit).await.unwrap(),
                Output::QueuedTurn(next)
            );
            client.execute(launch).await.unwrap();
        }
        assert_eq!(first.requests.lock().unwrap().len(), 1);
        assert_eq!(second.requests.lock().unwrap().len(), 1);
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn cancellation_preserves_partial_text() {
    for remote in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let profile = fixture.path().join("node");
        let node = Node::start_empty(&profile).await.unwrap();
        let controller =
            Link::controller(fixture.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let transport: Arc<dyn Transport> = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Client::new(transport);
        let server = Server::start(true).await;
        let (session, _) = configured_session(&client, &server.endpoint, &root).await;
        for stop_node in [false, true] {
            let Output::QueuedTurn(turn) = client
                .execute(client.prepare(Command::SubmitTurn {
                    session: session.id,
                    expected_revision: 1,
                    message: "slow prompt".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            server.wait_count(if stop_node { 2 } else { 1 }).await;
            let mut stream = client.subscribe_conversation(session.id).await.unwrap();
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    match stream.next().await.unwrap() {
                        Update::ConversationSnapshot(snapshot)
                            if snapshot.drafts.iter().any(|draft| {
                                draft.turn == turn.id
                                    && draft.parts == vec![Part::Text("partial-fixture".into())]
                            }) =>
                        {
                            break;
                        }
                        Update::ConversationFrame(frame)
                            if matches!(frame.change, sailry_protocol::conversation::Change::Delta(ref draft) if draft.turn == turn.id && draft.parts == vec![Part::Text("partial-fixture".into())]) => break,
                        _ => {}
                    }
                }
            })
            .await
            .unwrap();
            controller.handle().disconnect(node.id()).await;
            let page = history(&client, session.id).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Running);
            assert!(!page.entries.iter().any(|entry| entry.turn == turn.id
                && entry.parts == vec![Part::Text("partial-fixture".into())]));
            if stop_node {
                break;
            }
            let stop = client.prepare(Command::StopTurn { turn: turn.id });
            client.execute(stop.clone()).await.unwrap();
            let page = finished(&client, session.id, turn.id).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);
            assert_eq!(
                page.entries
                    .iter()
                    .filter(|entry| entry.turn == turn.id
                        && entry.parts == vec![Part::Text("partial-fixture".into())])
                    .count(),
                1
            );
            let mut resumed = client.subscribe_conversation(session.id).await.unwrap();
            let Update::ConversationSnapshot(snapshot) = resumed.next().await.unwrap() else {
                panic!("snapshot expected")
            };
            assert!(snapshot.drafts.is_empty());
            assert!(text(&snapshot.page).contains("partial-fixture"));
            client.execute(stop).await.unwrap();
        }
        tokio::time::timeout(Duration::from_secs(5), node.shutdown())
            .await
            .unwrap()
            .unwrap();
        controller.close().await.unwrap();
        let node = Node::start_empty(&profile).await.unwrap();
        let client = Client::new(node.local());
        let page = history(&client, session.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Interrupted);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        assert_eq!(
            page.entries
                .iter()
                .filter(|entry| entry.parts == vec![Part::Text("partial-fixture".into())])
                .count(),
            2
        );
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn serializes_turns_per_session() {
    for remote in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(fixture.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(fixture.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let transport: Arc<dyn Transport> = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Client::new(transport);
        let server = Server::start(true).await;
        let (first, _) = configured_session(&client, &server.endpoint, &root).await;
        let Output::Session(second) = client
            .execute(client.prepare(Command::CreateSession {
                project: first.project,
                worktree: Some(first.worktree),
                config: Some(first.config.clone()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let submit = |session| Command::SubmitTurn {
            session,
            expected_revision: 1,
            message: "serial fixture".into(),
        };
        let Output::QueuedTurn(active) = client
            .execute(client.prepare(submit(first.id)))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        server.wait_count(1).await;
        let Output::QueuedTurn(waiting) = client
            .execute(client.prepare(submit(first.id)))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        client
            .execute(client.prepare(submit(second.id)))
            .await
            .unwrap();
        server.wait_count(2).await;
        let page = history(&client, first.id).await;
        assert_eq!(page.runs[0].status, Status::Running);
        assert_eq!(page.runs[1].status, Status::Queued);
        client
            .execute(client.prepare(Command::StopTurn { turn: active.id }))
            .await
            .unwrap();
        let page = finished(&client, first.id, active.id).await;
        assert!(page.queue.paused);
        assert_eq!(page.runs[1].status, Status::Queued);
        client
            .execute(client.prepare(Command::SetQueuePaused {
                session: first.id,
                expected_revision: page.queue.revision,
                paused: false,
            }))
            .await
            .unwrap();
        server.wait_count(3).await;
        let page = history(&client, first.id).await;
        assert_eq!(page.runs[0].status, Status::Cancelled);
        assert_eq!(page.runs[1].turn, waiting.id);
        assert_eq!(page.runs[1].status, Status::Running);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn recovers_live_output() {
    for remote in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(fixture.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(fixture.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let transport: Arc<dyn Transport> = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Arc::new(Client::new(transport));
        let slow = Server::start(true).await;
        let fast = Server::start(false).await;
        let (session, mut provider) = configured_session(&client, &slow.endpoint, &root).await;
        let (updates, mut view) =
            tokio::sync::watch::channel(sailry_client::conversation::View::default());
        let stop = sailry_link::CancellationToken::new();
        let (_older, requests) = tokio::sync::mpsc::channel(1);
        let worker = tokio::spawn({
            let client = client.clone();
            let stop = stop.clone();
            async move {
                client
                    .watch_conversation(session.id, updates, stop, requests)
                    .await
            }
        });
        observed(&mut view, |view| view.snapshot.is_some() && view.connected).await;
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: 1,
                message: "streaming fixture".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        observed(&mut view, |view| {
            view.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .drafts
                    .iter()
                    .any(|draft| draft.parts == vec![Part::Text("partial-fixture".into())])
            })
        })
        .await;
        assert!(!text(&history(&client, session.id).await).contains("partial-fixture"));
        controller.handle().disconnect(node.id()).await;
        let mut resumed = client.subscribe_conversation(session.id).await.unwrap();
        let Update::ConversationSnapshot(recovered) = resumed.next().await.unwrap() else {
            panic!("snapshot expected")
        };
        assert_eq!(
            recovered.drafts[0].parts,
            vec![Part::Text("partial-fixture".into())]
        );
        assert_eq!(slow.requests.lock().unwrap().len(), 1);
        drop(resumed);
        client
            .execute(client.prepare(Command::StopTurn { turn: turn.id }))
            .await
            .unwrap();
        observed(&mut view, |view| {
            view.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.drafts.is_empty()
                    && snapshot
                        .page
                        .runs
                        .last()
                        .is_some_and(|run| run.status == Status::Cancelled)
            })
        })
        .await;
        provider.endpoint = fast.endpoint.clone();
        client
            .execute(client.prepare(Command::PutProvider {
                provider,
                expected_revision: 1,
            }))
            .await
            .unwrap();
        client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: 1,
                message: "completed fixture".into(),
            }))
            .await
            .unwrap();
        observed(&mut view, |view| {
            view.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .page
                    .runs
                    .last()
                    .is_some_and(|run| run.status == Status::Completed)
            })
        })
        .await;
        let page = history(&client, session.id).await;
        assert_eq!(
            view.borrow().snapshot.as_ref().unwrap().page.as_ref(),
            &page
        );
        assert!(view.borrow().snapshot.as_ref().unwrap().drafts.is_empty());
        assert!(text(&page).contains("answer-fixture-a"));
        assert_eq!(fast.requests.lock().unwrap().len(), 1);
        stop.cancel();
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!view.borrow().connected);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

async fn observed(
    view: &mut tokio::sync::watch::Receiver<sailry_client::conversation::View>,
    ready: impl Fn(&sailry_client::conversation::View) -> bool,
) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if ready(&view.borrow_and_update()) {
                break;
            }
            view.changed().await.expect("conversation observer ended");
        }
    })
    .await
    .expect("conversation update deadline");
}

#[path = "agent/evaluation.rs"]
mod evaluation;
#[path = "agent/live.rs"]
mod live;

#[path = "agent/smoke.rs"]
mod smoke;
