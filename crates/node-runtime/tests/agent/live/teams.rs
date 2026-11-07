//! Billable team execution verifies live scheduling with real model responses.
use super::compaction::Fixture;
use super::*;
use futures::FutureExt;
use std::panic::AssertUnwindSafe;

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

async fn observe(client: &Client, session: SessionId, ready: impl Fn(&Page) -> bool) -> Page {
    let mut updates = client.subscribe_conversation(session).await.unwrap();
    loop {
        let page = history(client, session).await;
        if ready(&page) {
            return page;
        }
        updates.next().await.expect("live Node subscription ended");
    }
}

async fn configure(fixture: &mut Fixture) -> Vec<role::Profile> {
    let mut config = fixture.session.config.clone();
    config.mode = WorkMode::Flagship;
    config.permission = Permission::Ask;
    let mut roles = Vec::new();
    for key in ["alpha", "beta"] {
        let profile = role::Profile {
            id: RoleId::new(),
            revision: 0,
            key: key.into(),
            name: key.into(),
            appearance: None,
            description: format!("Copy {key}.txt into {key}-copy.txt"),
            read_only: false,
            model: Some(role::Model {
                provider: config.provider,
                model: config.model.clone(),
                effort: Some(config.effort),
            }),
            max_turns: Some(6),
            skills: vec![],
            tools: Some(
                ["read_file", "write_file"]
                    .into_iter()
                    .map(|name| plugin::conversation::Tool::Package {
                        package: "files".into(),
                        name: name.into(),
                    })
                    .collect(),
            ),
            instructions: format!(
                "First use read_file to read {key}.txt, then use write_file to create {key}-copy.txt with exactly the same bytes, preserving the absence of a trailing newline. Use no shell or other file tools. Reply with the exact source contents after the write succeeds."
            ),
        };
        let Output::Role(role) = execute(
            &fixture.client,
            Command::PutRole {
                role: profile,
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("role expected")
        };
        roles.push(role);
    }
    let Output::Team(team) = execute(&fixture.client, Command::PutTeam {
        team: team::Profile {
            id: TeamId::new(), revision: 0, key: "acceptance".into(), name: "Acceptance".into(),
            appearance: None, description: "Copy two independent files".into(), model: None,
            instructions: "Delegate the two independent file tasks to their matching captured roles and collect both results".into(),
            members: roles.iter().map(|role| role.id).collect(),
        }, expected_revision: 0,
    }).await else { panic!("team expected") };
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
    let Output::Session(session) = execute(
        &fixture.client,
        Command::SetSessionExecution {
            execution: sailry_protocol::conversation::Execution::Team,
            models: Default::default(),
            session: session.id,
            expected_revision: session.revision,
            team: Some(team.reference().into()),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    fixture.session = session;
    assert_eq!(fixture.session.roles.profiles, roles);
    assert_eq!(fixture.session.roles.team, Some(team.reference()));
    roles
}

async fn capacity(client: &Client, count: u64) {
    let Output::TeamSettings(mut settings) = execute(client, Command::ReadTeamSettings).await
    else {
        panic!("team settings expected")
    };
    settings.child_concurrency = count;
    let Output::TeamSettings(saved) = execute(client, Command::SaveTeamSettings(settings)).await
    else {
        panic!("team settings expected")
    };
    assert_eq!(saved.child_concurrency, count);
}

fn completed(page: &Page, turn: TurnId) -> bool {
    page.runs.iter().any(|run| {
        run.turn == turn
            && !matches!(
                run.status,
                Status::Queued | Status::Running | Status::Stopping
            )
    })
}

async fn pending(client: &Client, session: SessionId) -> Approval {
    let page = observe(client, session, |page| {
        page.approvals
            .iter()
            .any(|approval| approval.state == ApprovalState::Pending)
    })
    .await;
    let approval = page
        .approvals
        .iter()
        .find(|approval| approval.state == ApprovalState::Pending)
        .unwrap();
    let entry = page
        .entries
        .iter()
        .find(|entry| entry.id == approval.entry)
        .unwrap();
    assert!(matches!(
        entry.parts.get(approval.index),
        Some(Part::ToolCall { name, .. }) if name == &plugin_tool("files", "write_file")
    ));
    approval.clone()
}

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

async fn exercise(fixture: &mut Fixture, submitted: &mut Option<TurnId>, stage: &mut &'static str) {
    let roles = configure(fixture).await;
    let expected: Vec<_> = roles
        .iter()
        .map(|role| {
            let contents = format!("{}-{}", role.key, uuid::Uuid::new_v4());
            std::fs::write(fixture.root().join(format!("{}.txt", role.key)), &contents).unwrap();
            contents
        })
        .collect();
    capacity(&fixture.client, 1).await;
    let tool = plugin_tool("files", "read_file");
    let Output::QueuedTurn(turn) = execute(&fixture.client, Command::SubmitTurn {
        session: fixture.session.id, expected_revision: fixture.session.revision,
        message: "Use spawn_agent exactly twice: once with role alpha and once with role beta, both with background=true. Each child must read its matching alpha.txt or beta.txt with read_file and write exactly those bytes to alpha-copy.txt or beta-copy.txt with write_file, without a trailing newline. Pass each child its own filenames and task explicitly. Start both children before waiting. Use wait_agent to collect both successful child results before your final answer, which must include both exact source contents. Do not read or write these files yourself, use shell commands, spawn extra children or finish while either task is pending.".into(),
    }).await else { panic!("turn expected") };
    *submitted = Some(turn.id);
    assert_eq!(turn.roles, fixture.session.roles);
    *stage = "queued child";
    let limited = observe(&fixture.client, fixture.session.id, |page| {
        page.children.len() == 2
            && page
                .children
                .iter()
                .filter(|child| child.run.status == Status::Running)
                .count()
                == 1
            && page
                .children
                .iter()
                .filter(|child| child.run.status == Status::Queued)
                .count()
                == 1
    })
    .await;
    let running = limited
        .children
        .iter()
        .find(|child| child.run.status == Status::Running)
        .unwrap();
    let queued = limited
        .children
        .iter()
        .find(|child| child.run.status == Status::Queued)
        .unwrap();
    *stage = "first child approval";
    let first = pending(&fixture.client, running.run.session).await;
    *stage = "capacity expansion";
    capacity(&fixture.client, 2).await;
    let second = pending(&fixture.client, queued.run.session).await;
    let expanded = observe(&fixture.client, fixture.session.id, |page| {
        page.children.len() == 2
            && page
                .children
                .iter()
                .all(|child| child.run.status == Status::Running)
    })
    .await;
    assert_ne!(first.session, second.session);
    for child in &expanded.children {
        assert_eq!(child.origin.turn, turn.id);
        assert!(roles.iter().any(|role| Some(role.id) == child.origin.role));
        let capture = fixture
            .client
            .read_turn_capture(child.run.session, child.run.turn)
            .await
            .unwrap();
        assert_eq!(capture.config.provider, turn.config.provider);
        assert_eq!(capture.config.model, turn.config.model);
        assert_eq!(capture.config.mode, WorkMode::Flagship);
    }
    assert_ne!(
        expanded.children[0].origin.role,
        expanded.children[1].origin.role
    );
    for role in &roles {
        assert!(
            !fixture
                .root()
                .join(format!("{}-copy.txt", role.key))
                .exists()
        );
    }
    for approval in [first, second] {
        execute(
            &fixture.client,
            Command::ResolveApproval {
                session: approval.session,
                approval: approval.id,
                decision: Decision::Approve,
            },
        )
        .await;
    }
    *stage = "child completion";
    for child in &expanded.children {
        let page = observe(&fixture.client, child.run.session, |page| {
            completed(page, child.run.turn)
        })
        .await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == child.run.turn)
                .unwrap()
                .status,
            Status::Completed
        );
        let index = roles
            .iter()
            .position(|role| Some(role.id) == child.origin.role)
            .unwrap();
        assert!(
            answer(&page).contains(&expected[index]),
            "child answer must contain the source token"
        );
        assert_eq!(
            std::fs::read_to_string(
                fixture
                    .root()
                    .join(format!("{}-copy.txt", roles[index].key))
            )
            .unwrap(),
            expected[index]
        );
    }
    *stage = "parent completion";
    let page = observe(&fixture.client, fixture.session.id, |page| {
        completed(page, turn.id)
    })
    .await;
    assert_eq!(
        page.runs
            .iter()
            .find(|run| run.turn == turn.id)
            .unwrap()
            .status,
        Status::Completed
    );
    let calls = tool_calls(&fixture.client, &page).await;
    let spawns: Vec<_> = calls
        .iter()
        .filter(|(name, _)| name == "spawn_agent")
        .collect();
    assert_eq!(spawns.len(), 2);
    assert!(
        spawns
            .iter()
            .all(|(_, detail)| detail.arguments["background"] == true)
    );
    assert!(calls.iter().any(|(name, _)| name == "wait_agent"));
    assert!(
        calls
            .iter()
            .all(|(name, _)| name != &tool && name != &plugin_tool("files", "write_file"))
    );
    for contents in expected {
        assert!(
            answer(&page).contains(&contents),
            "parent must collect both child answers"
        );
    }
}

#[tokio::test]
#[ignore = "requires an explicit inactive provider profile and incurs real model usage"]
async fn expands_capacity_and_collects_results() {
    let mut fixture = Fixture::new().await;
    let mut submitted = None;
    let mut stage = "configuration";
    let outcome = AssertUnwindSafe(tokio::time::timeout(
        Duration::from_secs(240),
        exercise(&mut fixture, &mut submitted, &mut stage),
    ))
    .catch_unwind()
    .await;
    if !matches!(&outcome, Ok(Ok(())))
        && let Some(turn) = submitted
    {
        let page = history(&fixture.client, fixture.session.id).await;
        eprintln!(
            "live team stopped: stage={stage} parent={:?} children={:?}",
            page.runs
                .iter()
                .map(|run| (run.status, run.error.as_ref().map(|error| error.code)))
                .collect::<Vec<_>>(),
            page.children
                .iter()
                .map(|child| (
                    child.run.status,
                    child.run.error.as_ref().map(|error| error.code)
                ))
                .collect::<Vec<_>>(),
        );
        let _ = tokio::time::timeout(
            Duration::from_secs(10),
            fixture
                .client
                .execute(fixture.client.prepare(Command::StopTurn { turn })),
        )
        .await;
    }
    fixture.close().await;
    match outcome {
        Ok(Ok(())) => println!(
            "live team: roles=2 concurrency=1-to-2 captures=verified files=verified results=collected"
        ),
        Ok(Err(error)) => panic!("live team deadline at {stage}: {error}"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
