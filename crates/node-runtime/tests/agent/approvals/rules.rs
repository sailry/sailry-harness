#![cfg(unix)]
use super::*;
use process::Fixture;
use sailry_protocol::{conversation::ApprovalChoice, plugin};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(crate) async fn observe(
    client: &Client,
    session: SessionId,
    predicate: impl Fn(&Page) -> bool,
) -> Page {
    observe_within(client, session, predicate, Duration::from_secs(10)).await
}

async fn observe_within(
    client: &Client,
    session: SessionId,
    predicate: impl Fn(&Page) -> bool,
    budget: Duration,
) -> Page {
    let mut last = None;
    let result = tokio::time::timeout(budget, async {
        let mut stream = client.subscribe_conversation(session).await.unwrap();
        let mut projection =
            sailry_client::conversation::Projection::new(client.target(), session, 0);
        loop {
            let update = stream.next().await.unwrap();
            assert_ne!(
                projection.apply(0, update).unwrap(),
                sailry_client::Apply::Recover
            );
            let Some(snapshot) = projection.snapshot() else {
                continue;
            };
            last = Some((
                snapshot
                    .page
                    .runs
                    .iter()
                    .map(|run| (run.turn, run.status))
                    .collect::<Vec<_>>(),
                snapshot.page.entries.len(),
                snapshot.page.approvals.len(),
                snapshot.page.questions.len(),
            ));
            assert!(
                snapshot
                    .page
                    .runs
                    .iter()
                    .all(|run| run.status != Status::Failed),
                "{:?}",
                snapshot.page.runs
            );
            if predicate(&snapshot.page) {
                return snapshot.page.as_ref().clone();
            }
        }
    })
    .await;
    result.unwrap_or_else(|error| {
        panic!(
            "conversation update deadline: {error:?}; last (runs, entries, approvals, questions): {last:?}"
        )
    })
}

pub(crate) async fn pending(client: &Client, session: SessionId) -> Approval {
    observe(client, session, |page| {
        page.approvals
            .iter()
            .any(|approval| approval.state == ApprovalState::Pending)
    })
    .await
    .approvals
    .into_iter()
    .find(|approval| approval.state == ApprovalState::Pending)
    .unwrap()
}

pub(crate) async fn completed(client: &Client, session: SessionId, turn: TurnId) -> Page {
    completed_within(client, session, turn, COMPLETION_BUDGET).await
}

pub(crate) async fn completed_within(
    client: &Client,
    session: SessionId,
    turn: TurnId,
    budget: Duration,
) -> Page {
    observe_within(
        client,
        session,
        |page| {
            page.runs
                .iter()
                .any(|run| run.turn == turn && run.status == Status::Completed)
        },
        budget,
    )
    .await;
    let page = history(client, session).await;
    assert!(
        page.runs
            .iter()
            .any(|run| run.turn == turn && run.status == Status::Completed)
    );
    page
}

pub(crate) async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

pub(crate) async fn submit(client: &Client, session: &Session) -> QueuedTurn {
    let Output::QueuedTurn(turn) = execute(
        client,
        Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Use the requested tool".into(),
        },
    )
    .await
    else {
        panic!("turn expected")
    };
    turn
}

pub(crate) async fn route(client: &Client, session: &Session, endpoint: &str) {
    let Output::Providers(providers) = execute(client, Command::ListProviders).await else {
        panic!("providers expected")
    };
    let mut provider = providers
        .into_iter()
        .find(|provider| provider.id == session.config.provider)
        .unwrap();
    let revision = provider.revision;
    provider.endpoint = endpoint.into();
    execute(
        client,
        Command::PutProvider {
            provider,
            expected_revision: revision,
        },
    )
    .await;
}

pub(crate) async fn restart(mut fixture: Fixture, remote: bool) -> Fixture {
    let profile = fixture.node.profile().to_owned();
    let id = fixture.node.id();
    fixture.node.shutdown().await.unwrap();
    fixture.controller.handle().disconnect(id).await;
    fixture.node = Node::start_empty(profile).await.unwrap();
    fixture.client = Client::new(if remote {
        fixture
            .controller
            .handle()
            .remote(fixture.node.link().address())
    } else {
        fixture.node.local()
    });
    fixture
}

pub(crate) async fn claimed(node: &Node, approval: &Approval) -> (RequestId, bool) {
    let id = approval.id;
    let (request, claimed): (String, bool) = node
        .with_storage(move |database| {
            Box::pin(async move {
                let row = database
                    .prepare("SELECT request,claimed FROM agent_approvals WHERE id=?1")
                    .await
                    .map_err(crate::database_fixture::fault)?
                    .query_row([id.to_string()])
                    .await
                    .map_err(crate::database_fixture::fault)?;
                Ok((
                    row.get(0).map_err(crate::database_fixture::fault)?,
                    row.get(1).map_err(crate::database_fixture::fault)?,
                ))
            })
        })
        .await
        .unwrap();
    (request.parse().unwrap(), claimed)
}

pub(crate) async fn results(
    client: &Client,
    page: &Page,
    turn: TurnId,
) -> Vec<(Option<String>, String, Value)> {
    let mut results = Vec::new();
    for entry in page.entries.iter().filter(|entry| entry.turn == turn) {
        for (index, part) in entry.parts.iter().enumerate() {
            if let Part::ToolResult { id, name, .. } = part {
                results.push((
                    id.clone(),
                    name.clone(),
                    tool_detail(client, page, entry, index)
                        .await
                        .result
                        .unwrap(),
                ));
            }
        }
    }
    results
}

#[tokio::test]
async fn releases_matching_pending_calls() {
    for remote in [false, true] {
        let write = plugin_tool("files", "write_file");
        let mut calls = vec![(
            plugin_tool("files", "create_directory"),
            json!({"path":"waiting"}),
        )];
        calls.extend(["first", "second", "third"].map(|name| {
            (
                write.clone(),
                json!({"path":format!("{name}.txt"),"text":name,"expected_revision":null}),
            )
        }));
        let server = Server::batch(calls.clone()).await;
        let fixture = Fixture::packages(remote, &server, &["files"]).await;
        let turn = submit(&fixture.client, &fixture.session).await;
        let unrelated = pending(&fixture.client, fixture.session.id).await;
        let Output::Plugin(package) = execute(
            &fixture.client,
            Command::ReadPlugin {
                name: "files".into(),
            },
        )
        .await
        else {
            panic!("plugin expected")
        };
        let context = plugin::Context {
            invocation: None,
            turn: Some(turn.id),
            surface: plugin::desktop::Surface::Workspace,
            package: package.summary.reference(),
            worktree: Some(fixture.session.worktree),
            session: Some(fixture.session.id),
        };
        // Keep the first unsafe call paused while the real Node owns concurrent
        // waiters for the other canonical calls in this captured model batch.
        let waiters = calls
            .iter()
            .enumerate()
            .skip(1)
            .map(|(index, (name, args))| {
                let request = fixture
                    .client
                    .prepare(Command::WriteFile {
                        worktree: fixture.session.worktree,
                        path: args["path"].as_str().unwrap().into(),
                        text: args["text"].as_str().unwrap().into(),
                        expected_revision: None,
                    })
                    .with_plugin(context.clone());
                fixture.node.authorize_tool_fixture(
                    Some(format!("call-fixture-{index}")),
                    name.clone(),
                    args.clone(),
                    sailry_protocol::tool::Operation::WriteFile,
                    request,
                )
            });
        let mut waiters = Box::pin(futures::future::join_all(waiters));
        let page = tokio::select! {
            page = observe(&fixture.client, fixture.session.id, |page| {
                page.approvals.len() == 4
                    && page.approvals.iter().all(|approval| approval.state == ApprovalState::Pending)
            }) => page,
            replies = &mut waiters => panic!("pending waiters completed early: {replies:?}"),
        };
        let matching: Vec<_> = page
            .approvals
            .iter()
            .filter(|approval| approval.id != unrelated.id)
            .cloned()
            .collect();
        assert_eq!(matching.len(), 3);
        assert!(matching.iter().all(|approval| approval.can_remember));
        for name in ["first", "second", "third"] {
            assert!(!fixture.root.join(format!("{name}.txt")).exists());
        }
        process::decide(&fixture.client, &matching[0], Decision::ApproveSession).await;
        let requests = tokio::time::timeout(Duration::from_secs(10), waiters)
            .await
            .expect("matching approval waiter deadline")
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let outputs = futures::future::join_all(
            requests
                .iter()
                .map(|request| fixture.client.execute(request.clone())),
        )
        .await;
        for (request, output) in requests.iter().zip(outputs) {
            let output = output.unwrap();
            assert!(matches!(output, Output::FileWritten(_)));
            assert_eq!(
                fixture.client.outcome(request).await.unwrap(),
                RequestOutcome::Completed(Box::new(Ok(output)))
            );
        }
        let page = observe(&fixture.client, fixture.session.id, |page| {
            matching.iter().all(|pending| {
                page.approvals.iter().any(|approval| {
                    approval.id == pending.id && approval.state == ApprovalState::Approved
                })
            })
        })
        .await;
        let request_ids: std::collections::BTreeSet<_> =
            requests.iter().map(|request| request.id).collect();
        assert_eq!(request_ids.len(), 3);
        for pending in &matching {
            let approval = page
                .approvals
                .iter()
                .find(|approval| approval.id == pending.id)
                .unwrap();
            assert_eq!(
                approval.source,
                if approval.id == matching[0].id {
                    ApprovalSource::User
                } else {
                    ApprovalSource::SessionRule
                }
            );
            assert_eq!(approval.choice, Some(ApprovalChoice::Session));
            let (request, consumed) = claimed(&fixture.node, approval).await;
            assert!(consumed);
            assert!(request_ids.contains(&request));
        }
        assert_eq!(
            page.approvals
                .iter()
                .find(|approval| approval.id == unrelated.id)
                .unwrap(),
            &unrelated
        );
        assert!(!claimed(&fixture.node, &unrelated).await.1);
        assert!(!fixture.root.join("waiting").exists());
        for name in ["first", "second", "third"] {
            assert_eq!(
                std::fs::read_to_string(fixture.root.join(format!("{name}.txt"))).unwrap(),
                name
            );
        }
        execute(&fixture.client, Command::StopTurn { turn: turn.id }).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Cancelled);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn remembers_across_turns_and_restart() {
    for remote in [false, true] {
        let first_server = Server::turn_tools(vec![(
            plugin_tool("files", "write_file"),
            json!({"path":"first.txt","text":"first","expected_revision":null}),
        )])
        .await;
        let second_server = Server::turn_tools(vec![(
            plugin_tool("files", "write_file"),
            json!({"path":"second.txt","text":"second","expected_revision":null}),
        )])
        .await;
        let third_server = Server::turn_tools(vec![(
            plugin_tool("files", "write_file"),
            json!({"path":"third.txt","text":"third","expected_revision":null}),
        )])
        .await;
        let mut fixture = Fixture::packages(remote, &first_server, &["files"]).await;
        let first = submit(&fixture.client, &fixture.session).await;
        let approval = pending(&fixture.client, fixture.session.id).await;
        assert!(approval.can_remember);
        assert!(!fixture.root.join("first.txt").exists());
        let (decision, receipt) =
            process::decide(&fixture.client, &approval, Decision::ApproveSession).await;
        completed(&fixture.client, fixture.session.id, first.id).await;
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("first.txt")).unwrap(),
            "first"
        );
        route(&fixture.client, &fixture.session, &second_server.endpoint).await;
        let second = submit(&fixture.client, &fixture.session).await;
        completed(&fixture.client, fixture.session.id, second.id).await;
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("second.txt")).unwrap(),
            "second"
        );
        fixture = restart(fixture, remote).await;
        route(&fixture.client, &fixture.session, &third_server.endpoint).await;
        let third = submit(&fixture.client, &fixture.session).await;
        let page = completed(&fixture.client, fixture.session.id, third.id).await;
        assert_eq!(page.approvals.len(), 3);
        let mut requests = std::collections::BTreeSet::new();
        let mut approvals = std::collections::BTreeSet::new();
        for (turn, source) in [
            (first.id, ApprovalSource::User),
            (second.id, ApprovalSource::SessionRule),
            (third.id, ApprovalSource::SessionRule),
        ] {
            let approval = page
                .approvals
                .iter()
                .find(|approval| approval.turn == turn)
                .unwrap();
            assert_eq!(approval.state, ApprovalState::Approved);
            assert_eq!(approval.source, source);
            assert_eq!(approval.choice, Some(ApprovalChoice::Session));
            let (request, consumed) = claimed(&fixture.node, approval).await;
            assert!(consumed);
            assert!(requests.insert(request));
            assert!(approvals.insert(approval.id));
            let results = results(&fixture.client, &page, turn).await;
            assert_eq!(results.len(), 1);
            assert!(results[0].2.get("error").is_none(), "{:?}", results[0]);
        }
        assert_eq!(fixture.client.execute(decision).await.unwrap(), receipt);
        for (name, content) in [("first", "first"), ("second", "second"), ("third", "third")] {
            assert_eq!(
                std::fs::read_to_string(fixture.root.join(format!("{name}.txt"))).unwrap(),
                content
            );
        }
        assert_eq!(
            [first_server, second_server, third_server]
                .iter()
                .map(|server| server.requests.lock().unwrap().len())
                .sum::<usize>(),
            6
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn keeps_plain_approval_once() {
    for remote in [false, true] {
        let server = Server::tools(vec![
            (
                plugin_tool("files", "write_file"),
                json!({"path":"count.txt","text":"x","expected_revision":null}),
            ),
            (
                plugin_tool("files", "write_file"),
                json!({"path":"count.txt","text":"x","expected_revision":null}),
            ),
        ])
        .await;
        let fixture = Fixture::packages(remote, &server, &["files"]).await;
        let turn = submit(&fixture.client, &fixture.session).await;
        let first = pending(&fixture.client, fixture.session.id).await;
        let (decision, receipt) = process::decide(&fixture.client, &first, Decision::Approve).await;
        let second = pending(&fixture.client, fixture.session.id).await;
        assert_ne!(first.id, second.id);
        assert_eq!(second.source, ApprovalSource::User);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("count.txt")).unwrap(),
            "x"
        );
        assert_eq!(fixture.client.execute(decision).await.unwrap(), receipt);
        assert_eq!(pending(&fixture.client, fixture.session.id).await, second);
        process::decide(&fixture.client, &second, Decision::Deny).await;
        let page = completed(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.approvals[0].choice, Some(ApprovalChoice::Once));
        assert!(claimed(&fixture.node, &first).await.1);
        assert!(!claimed(&fixture.node, &second).await.1);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("count.txt")).unwrap(),
            "x"
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn isolates_sessions_forks_and_worktrees() {
    for remote in [false, true] {
        let server = Server::turn_tools(vec![(
            plugin_tool("files", "write_file"),
            json!({"path":"count.txt","text":"x","expected_revision":null}),
        )])
        .await;
        let mut fixture = Fixture::packages(remote, &server, &["files"]).await;
        let first = submit(&fixture.client, &fixture.session).await;
        let approval = pending(&fixture.client, fixture.session.id).await;
        process::decide(&fixture.client, &approval, Decision::ApproveSession).await;
        completed(&fixture.client, fixture.session.id, first.id).await;
        let Output::Session(other) = execute(
            &fixture.client,
            Command::CreateSession {
                project: fixture.session.project,
                worktree: Some(fixture.session.worktree),
                config: Some(fixture.session.config.clone()),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        let Output::Session(fork) = execute(
            &fixture.client,
            Command::ForkConversation {
                session: fixture.session.id,
                through: first.id,
                expected_revision: fixture.session.revision,
            },
        )
        .await
        else {
            panic!("fork expected")
        };
        for session in [other, fork] {
            let turn = submit(&fixture.client, &session).await;
            let pending = pending(&fixture.client, session.id).await;
            assert_eq!(pending.session, session.id);
            assert_eq!(pending.source, ApprovalSource::User);
            assert_eq!(
                std::fs::read_to_string(fixture.root.join("count.txt")).unwrap(),
                "x"
            );
            process::decide(&fixture.client, &pending, Decision::Deny).await;
            completed(&fixture.client, session.id, turn.id).await;
        }
        let repository = git2::Repository::init(&fixture.root).unwrap();
        std::fs::write(fixture.root.join("seed.txt"), "Linked worktree fixture").unwrap();
        let mut index = repository.index().unwrap();
        index.add_path(std::path::Path::new("seed.txt")).unwrap();
        index.write().unwrap();
        let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = git2::Signature::now("Fixture", "fixture@example.com").unwrap();
        repository
            .commit(Some("HEAD"), &signature, &signature, "Fixture", &tree, &[])
            .unwrap();
        let linked = fixture.directory.path().join("linked");
        repository.worktree("linked", &linked, None).unwrap();
        let Output::Worktree(worktree) = execute(
            &fixture.client,
            Command::RegisterWorktree {
                project: fixture.session.project.unwrap(),
                path: linked.to_str().unwrap().into(),
            },
        )
        .await
        else {
            panic!("worktree expected")
        };
        let Output::Session(session) = execute(
            &fixture.client,
            Command::MoveConversation {
                session: fixture.session.id,
                worktree: worktree.id,
                expected_revision: fixture.session.revision,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let turn = submit(&fixture.client, &fixture.session).await;
        let pending = pending(&fixture.client, fixture.session.id).await;
        assert_eq!(pending.source, ApprovalSource::User);
        assert!(!linked.join("count.txt").exists());
        process::decide(&fixture.client, &pending, Decision::Deny).await;
        completed(&fixture.client, fixture.session.id, turn.id).await;
        assert!(!linked.join("count.txt").exists());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

async fn package(fixture: &Fixture, revision: u64, version: &str) -> plugin::Info {
    let root = fixture.root.join("package");
    std::fs::create_dir_all(root.join(plugin::NAMESPACE)).unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/settings.json"),
        json!({
            "$schema":plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
            "properties":{"label":{"type":"string","default":"Initial"}},"required":["label"],
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(root.join("plugin.json"), json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name":"approval-fixture","version":version,"extensions":{"dev.sailry.platform":{
            "api_version":"v1","actions":["files.write"],"settings_schema":"dev.sailry.platform/settings.json",
            "tools":[{"name":"write","operation":"files.write"},{"name":"other_write","operation":"files.write"}],
        }},
    }).to_string()).unwrap();
    let Output::Plugin(info) = execute(
        &fixture.client,
        Command::InstallPlugin {
            name: "approval-fixture".into(),
            path: "package".into(),
            worktree: fixture.session.worktree,
            expected_revision: revision,
            expected_digest: None,
        },
    )
    .await
    else {
        panic!("plugin expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    info
}

#[tokio::test]
async fn binds_package_settings_and_tool() {
    for remote in [false, true] {
        let write = plugin_tool("approval-fixture", "write");
        let server = Server::turn_tools(vec![(
            write.clone(),
            json!({"path":"same.txt","text":"first","expected_revision":null}),
        )])
        .await;
        let fixture = Fixture::packages(remote, &server, &[]).await;
        let old = package(&fixture, 0, "0.1.0").await;
        let first = submit(&fixture.client, &fixture.session).await;
        let approval = pending(&fixture.client, fixture.session.id).await;
        assert!(approval.can_remember);
        process::decide(&fixture.client, &approval, Decision::ApproveSession).await;
        completed(&fixture.client, fixture.session.id, first.id).await;
        let Output::QueuedTurn(queued) = execute(
            &fixture.client,
            Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Use frozen package settings".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        assert!(queued.plugins.contains(&old.summary.reference()));
        let Output::PluginSettings(settings) = execute(
            &fixture.client,
            Command::SavePluginSettings {
                package: old.summary.reference(),
                values: BTreeMap::from([("label".into(), json!("Changed"))]),
                secrets: BTreeMap::new(),
            },
        )
        .await
        else {
            panic!("settings expected")
        };
        assert_ne!(
            settings.package.settings_revision,
            old.summary.settings_revision
        );
        execute(
            &fixture.client,
            Command::StartQueuedTurn { turn: queued.id },
        )
        .await;
        let page = completed(&fixture.client, fixture.session.id, queued.id).await;
        let retained = page
            .approvals
            .iter()
            .find(|approval| approval.turn == queued.id)
            .unwrap();
        assert_eq!(retained.source, ApprovalSource::SessionRule);
        assert!(claimed(&fixture.node, retained).await.1);

        let changed = submit(&fixture.client, &fixture.session).await;
        assert!(changed.plugins.contains(&settings.package));
        let approval = pending(&fixture.client, fixture.session.id).await;
        assert_eq!(approval.source, ApprovalSource::User);
        process::decide(&fixture.client, &approval, Decision::Deny).await;
        completed(&fixture.client, fixture.session.id, changed.id).await;
        let Output::Plugin(current) = execute(
            &fixture.client,
            Command::ReadPlugin {
                name: "approval-fixture".into(),
            },
        )
        .await
        else {
            panic!("plugin expected")
        };
        let updated = package(&fixture, current.summary.revision, "0.1.1").await;
        assert_ne!(updated.summary.digest, old.summary.digest);
        let turn = submit(&fixture.client, &fixture.session).await;
        let approval = pending(&fixture.client, fixture.session.id).await;
        assert_eq!(approval.source, ApprovalSource::User);
        process::decide(&fixture.client, &approval, Decision::ApproveSession).await;
        completed(&fixture.client, fixture.session.id, turn.id).await;
        let other = Server::turn_tools(vec![(
            plugin_tool("approval-fixture", "other_write"),
            json!({"path":"other.txt","text":"must not execute","expected_revision":null}),
        )])
        .await;
        route(&fixture.client, &fixture.session, &other.endpoint).await;
        let turn = submit(&fixture.client, &fixture.session).await;
        let approval = pending(&fixture.client, fixture.session.id).await;
        assert_eq!(approval.source, ApprovalSource::User);
        assert!(!fixture.root.join("other.txt").exists());
        process::decide(&fixture.client, &approval, Decision::Deny).await;
        completed(&fixture.client, fixture.session.id, turn.id).await;
        assert!(!fixture.root.join("other.txt").exists());
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("same.txt")).unwrap(),
            "first"
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn isolates_delegated_children() {
    for remote in [false, true] {
        let server = Server::turn_tools(vec![(
            plugin_tool("files", "write_file"),
            json!({"path":"count.txt","text":"x","expected_revision":null}),
        )])
        .await;
        let mut fixture = Fixture::packages(remote, &server, &["files", "delegation"]).await;
        let first = submit(&fixture.client, &fixture.session).await;
        let approval = pending(&fixture.client, fixture.session.id).await;
        process::decide(&fixture.client, &approval, Decision::ApproveSession).await;
        completed(&fixture.client, fixture.session.id, first.id).await;
        let child = Server::turn_tools(vec![(
            plugin_tool("files", "write_file"),
            json!({"path":"count.txt","text":"x","expected_revision":null}),
        )])
        .await;
        let Output::Providers(providers) = execute(&fixture.client, Command::ListProviders).await
        else {
            panic!("providers expected")
        };
        let mut provider = providers
            .into_iter()
            .find(|provider| provider.id == fixture.session.config.provider)
            .unwrap();
        provider.id = ProviderId::new();
        provider.revision = 0;
        provider.endpoint = child.endpoint.clone();
        let Output::Provider(provider) = execute(
            &fixture.client,
            Command::PutProvider {
                provider,
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let role = role::Profile {
            read_only: false,
            id: RoleId::new(),
            revision: 0,
            key: "review".into(),
            name: "Review".into(),
            appearance: None,
            description: "Run the delegated fixture".into(),
            model: Some(role::Model {
                provider: provider.id,
                model: "fixture-a".into(),
                effort: None,
            }),
            max_turns: Some(5),
            skills: vec![],
            tools: None,
            instructions: "Use the requested tool".into(),
        };
        let Output::Role(role) = execute(
            &fixture.client,
            Command::PutRole {
                role,
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("role expected")
        };
        let Output::Session(session) = execute(
            &fixture.client,
            Command::SetSessionRoles {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                roles: vec![role.reference()],
            },
        )
        .await
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let parent = Server::turn_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"review","task":"Run the requested command"}),
        )])
        .await;
        route(&fixture.client, &fixture.session, &parent.endpoint).await;
        let turn = submit(&fixture.client, &fixture.session).await;
        let page = observe(&fixture.client, fixture.session.id, |page| {
            !page.children.is_empty()
        })
        .await;
        let child_session = page.children.last().unwrap().run.session;
        let approval = pending(&fixture.client, child_session).await;
        assert_ne!(approval.session, fixture.session.id);
        assert_eq!(approval.source, ApprovalSource::User);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("count.txt")).unwrap(),
            "x"
        );
        process::decide(&fixture.client, &approval, Decision::Deny).await;
        completed(&fixture.client, fixture.session.id, turn.id).await;
        completed(&fixture.client, child_session, approval.turn).await;
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("count.txt")).unwrap(),
            "x"
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
