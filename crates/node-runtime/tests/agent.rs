#[path = "agent/activity.rs"]
mod activity;
#[expect(
    dead_code,
    reason = "The shared model fixture includes response modes used by other integration test binaries"
)]
mod agent_support;
#[path = "agent/approvals.rs"]
mod approvals;
#[path = "agent/browser.rs"]
mod browser;
#[path = "agent/checkpoints.rs"]
mod checkpoints;
#[cfg(unix)]
#[path = "agent/compaction.rs"]
mod compaction;
#[path = "agent/computer.rs"]
mod computer;
#[path = "agent/configuration.rs"]
mod configuration;
#[path = "agent/connections.rs"]
mod connections;
#[path = "agent/delegation/mod.rs"]
mod delegation;
#[path = "agent/external_browser.rs"]
mod external_browser;
#[path = "agent/forks.rs"]
mod forks;
#[cfg(unix)]
#[path = "agent/goals.rs"]
mod goals;
#[cfg(unix)]
#[path = "agent/mcp/mod.rs"]
mod mcp;
#[path = "agent/media.rs"]
mod media;
#[cfg(unix)]
#[path = "agent/memory.rs"]
mod memory;
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
#[path = "agent/rewind.rs"]
mod rewind;
#[path = "agent/search.rs"]
mod search;
#[path = "agent/skills/mod.rs"]
mod skills;
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
#[path = "agent/worktrees.rs"]
mod worktrees;

use agent_support::*;
use sailry_client::Client;
use sailry_link::{Link, NetworkScope, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::{conversation::*, *};
use std::{sync::Arc, time::Duration};

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
    let provider = Provider {
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
                assistant: None,
                resource: None,
                provider: provider.id,
                model: "fixture-a".into(),
                effort: Effort::High,
                mode: sailry_protocol::WorkMode::Code,
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

async fn finished(client: &Client, session: SessionId, turn: TurnId) -> Page {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
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
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Agent completion deadline")
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
        let node = Node::start(&profile).await.unwrap();
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
        let (session, mut provider) = configured(&client, &first.endpoint, &root).await;
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
                provider,
                expected_revision: 1,
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
        assert_eq!(recent.page.entries.len(), 2);
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
        let node = Node::start(&profile).await.unwrap();
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
async fn cancellation_discards_partial_text() {
    for remote in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let profile = fixture.path().join("node");
        let node = Node::start(&profile).await.unwrap();
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
        let (session, _) = configured(&client, &server.endpoint, &root).await;
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
            controller.handle().disconnect(node.id()).await;
            let page = history(&client, session.id).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Running);
            assert!(!text(&page).contains("partial-fixture"));
            if stop_node {
                break;
            }
            let stop = client.prepare(Command::StopTurn { turn: turn.id });
            client.execute(stop.clone()).await.unwrap();
            let page = finished(&client, session.id, turn.id).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);
            assert!(!text(&page).contains("partial-fixture"));
            client.execute(stop).await.unwrap();
        }
        tokio::time::timeout(Duration::from_secs(5), node.shutdown())
            .await
            .unwrap()
            .unwrap();
        controller.close().await.unwrap();
        let node = Node::start(&profile).await.unwrap();
        let client = Client::new(node.local());
        let page = history(&client, session.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Interrupted);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        assert!(!text(&page).contains("partial-fixture"));
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn serializes_turns_per_session() {
    for remote in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start(fixture.path().join("node")).await.unwrap();
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
        let (first, _) = configured(&client, &server.endpoint, &root).await;
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
        let node = Node::start(fixture.path().join("node")).await.unwrap();
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
        let (session, mut provider) = configured(&client, &slow.endpoint, &root).await;
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
