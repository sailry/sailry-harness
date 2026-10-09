#![cfg(any(target_os = "macos", target_os = "linux"))]
use super::*;
use process::Fixture;
use sailry_protocol::{
    hooks::{Script, Settings},
    process::Outcome,
};
use serde_json::json;

#[path = "hooks/failures.rs"]
mod failures;
#[path = "hooks/recovery.rs"]
mod recovery;

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

fn script(command: &str) -> Option<Script> {
    Some(Script {
        command: command.into(),
        timeout_ms: 5_000,
    })
}

async fn read(client: &Client, worktree: WorktreeId) -> Settings {
    let Output::HookSettings(state) = execute(client, Command::ReadHooks { worktree }).await else {
        panic!("hook settings expected")
    };
    assert_eq!(state.worktree, worktree);
    assert!(state.sandboxed);
    state.settings
}

async fn save(client: &Client, worktree: WorktreeId, settings: Settings) -> Settings {
    let Output::HookSettings(state) = execute(
        client,
        Command::SaveHooks {
            worktree,
            expected_revision: settings.revision,
            settings,
        },
    )
    .await
    else {
        panic!("hook settings expected")
    };
    assert_eq!(state.worktree, worktree);
    state.settings
}

async fn ready(fixture: &mut Fixture, mode: WorkMode) {
    planning::configure(fixture, mode).await;
}

async fn file(path: &std::path::Path) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("hook fixture file deadline");
}

#[tokio::test]
async fn requires_native_controller_consent() {
    for remote in [false, true] {
        let server = Server::start(false).await;
        let fixture = Fixture::new(remote, &server).await;
        let settings = Settings {
            enabled: true,
            before_tool: script("printf enabled > enabled.txt"),
            ..Default::default()
        };
        let request = fixture.client.prepare(Command::SaveHooks {
            worktree: fixture.session.worktree,
            expected_revision: 0,
            settings: settings.clone(),
        });
        let first = fixture.client.execute(request.clone()).await.unwrap();
        assert_eq!(fixture.client.execute(request).await.unwrap(), first);
        assert_eq!(
            read(&fixture.client, fixture.session.worktree)
                .await
                .revision,
            1
        );
        assert!(!fixture.root.join("enabled.txt").exists());
        let stale = fixture
            .client
            .execute(fixture.client.prepare(Command::SaveHooks {
                worktree: fixture.session.worktree,
                expected_revision: 0,
                settings: settings.clone(),
            }))
            .await
            .unwrap_err();
        assert_eq!(stale.code, ErrorCode::RevisionConflict);
        let Output::Plugin(package) = execute(
            &fixture.client,
            Command::ReadPlugin {
                name: "commands".into(),
            },
        )
        .await
        else {
            panic!("package expected")
        };
        for turn in [None, Some(TurnId::new())] {
            let context = plugin::Context {
                package: package.summary.reference(),
                worktree: Some(fixture.session.worktree),
                session: Some(fixture.session.id),
                turn,
                invocation: None,
                surface: Default::default(),
            };
            for command in [
                Command::ReadHooks {
                    worktree: fixture.session.worktree,
                },
                Command::SaveHooks {
                    worktree: fixture.session.worktree,
                    expected_revision: 1,
                    settings: Settings {
                        revision: 1,
                        ..settings.clone()
                    },
                },
            ] {
                let error = fixture
                    .client
                    .execute(fixture.client.prepare(command).with_plugin(context.clone()))
                    .await
                    .unwrap_err();
                assert_eq!(error.code, ErrorCode::PermissionDenied);
            }
        }
        assert_eq!(
            read(&fixture.client, fixture.session.worktree)
                .await
                .revision,
            1
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn orders_callbacks_in_worktree_scope() {
    for remote in [false, true] {
        let tool = plugin_tool("commands", "run_command");
        let server=Server::tools(vec![(tool.clone(),json!({"command":"printf 'tool\\n' >> order.txt","description":"Write the fixture marker"}))]).await;
        let mut fixture = Fixture::new(remote, &server).await;
        ready(&mut fixture, WorkMode::Code).await;
        let before = format!(
            "test \"$SAILRY_HOOK_EVENT\" = before_tool && test \"$SAILRY_HOOK_TOOL\" = '{tool}' && test \"$SAILRY_HOOK_WORKTREE\" = '{}' && test -z \"${{OPENAI_API_KEY+x}}\" && test -z \"${{SAILRY_HOOK_ARGS+x}}\" && /bin/pwd > cwd.txt && printf 'before\\n' >> order.txt",
            fixture.session.worktree
        );
        save(
            &fixture.client,
            fixture.session.worktree,
            Settings {
                enabled: true,
                before_tool: script(&before),
                after_tool: script(
                    "test \"$SAILRY_HOOK_STATUS\" = succeeded && printf 'after\\n' >> order.txt",
                ),
                turn_end: script(
                    "test \"$SAILRY_HOOK_STATUS\" = completed && printf 'end\\n' >> order.txt",
                ),
                ..Default::default()
            },
        )
        .await;
        let turn = approvals::rules::submit(&fixture.client, &fixture.session).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Completed,
            "{page:?}"
        );
        assert_eq!(process::results(&page)[0].outcome, Outcome::Exited(0));
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("order.txt")).unwrap(),
            "before\ntool\nafter\nend\n"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("cwd.txt"))
                .unwrap()
                .trim(),
            fixture.root.canonicalize().unwrap().to_str().unwrap()
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn freezes_settings_before_queued_execution() {
    for remote in [false, true] {
        let server = Server::tools(vec![(
            plugin_tool("commands", "run_command"),
            json!({"command":"printf tool >> order.txt"}),
        )])
        .await;
        let mut fixture = Fixture::new(remote, &server).await;
        ready(&mut fixture, WorkMode::Code).await;
        let first = save(
            &fixture.client,
            fixture.session.worktree,
            Settings {
                enabled: true,
                before_tool: script("printf admitted >> order.txt"),
                turn_end: script("printf end >> order.txt"),
                ..Default::default()
            },
        )
        .await;
        let Output::QueuedTurn(turn) = execute(
            &fixture.client,
            Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Run the fixture command".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        save(
            &fixture.client,
            fixture.session.worktree,
            Settings {
                revision: first.revision,
                enabled: false,
                before_tool: script("printf changed >> order.txt"),
                ..Default::default()
            },
        )
        .await;
        ready(&mut fixture, WorkMode::Plan).await;
        execute(&fixture.client, Command::StartQueuedTurn { turn: turn.id }).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Completed
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("order.txt")).unwrap(),
            "admittedtoolend"
        );
        assert!(
            !read(&fixture.client, fixture.session.worktree)
                .await
                .enabled
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn reports_failures_without_replaying_tools() {
    for remote in [false, true] {
        for phase in ["before", "after", "end"] {
            let server = Server::tools(vec![(
                plugin_tool("commands", "run_command"),
                json!({"command":"printf once >> actual.txt"}),
            )])
            .await;
            let mut fixture = Fixture::new(remote, &server).await;
            ready(&mut fixture, WorkMode::Code).await;
            let failure = script("printf failure >> hooks.txt; exit 9");
            let settings = Settings {
                enabled: true,
                before_tool: (phase == "before").then(|| failure.clone()).flatten(),
                after_tool: (phase == "after").then(|| failure.clone()).flatten(),
                turn_end: if phase == "end" {
                    failure
                } else {
                    script("printf end >> hooks.txt")
                },
                ..Default::default()
            };
            save(&fixture.client, fixture.session.worktree, settings).await;
            let turn = approvals::rules::submit(&fixture.client, &fixture.session).await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            if phase == "before" {
                assert!(!fixture.root.join("actual.txt").exists());
                assert!(page.entries.iter().flat_map(|entry|&entry.parts).any(|part|matches!(part,Part::ToolResult {result,..} if result.to_string().contains("before_tool hook failed"))));
            } else {
                assert_eq!(
                    std::fs::read_to_string(fixture.root.join("actual.txt")).unwrap(),
                    "once"
                );
                assert_eq!(process::results(&page)[0].outcome, Outcome::Exited(0));
                let run = page.runs.iter().find(|run| run.turn == turn.id).unwrap();
                assert_eq!(run.status, Status::Failed);
                assert!(
                    run.error
                        .as_ref()
                        .unwrap()
                        .message
                        .contains(if phase == "after" {
                            "after_tool"
                        } else {
                            "turn_end"
                        })
                );
            }
            assert_eq!(
                std::fs::read_to_string(fixture.root.join("hooks.txt"))
                    .unwrap()
                    .matches("failure")
                    .count(),
                1
            );
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn cancels_and_bounds_hook_processes() {
    for remote in [false, true] {
        for cancel in [false, true] {
            let server = Server::tools(vec![(
                plugin_tool("commands", "run_command"),
                json!({"command":"printf tool > forbidden.txt"}),
            )])
            .await;
            let mut fixture = Fixture::new(remote, &server).await;
            ready(&mut fixture, WorkMode::Code).await;
            save(
                &fixture.client,
                fixture.session.worktree,
                Settings {
                    enabled: true,
                    before_tool: Some(Script {
                        command:
                            "printf started > started.txt; /bin/sleep 2; printf late > late.txt"
                                .into(),
                        timeout_ms: if cancel { 5_000 } else { 50 },
                    }),
                    ..Default::default()
                },
            )
            .await;
            let turn = approvals::rules::submit(&fixture.client, &fixture.session).await;
            if cancel {
                file(&fixture.root.join("started.txt")).await;
                execute(&fixture.client, Command::StopTurn { turn: turn.id }).await;
            }
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            if cancel {
                assert_eq!(
                    page.runs
                        .iter()
                        .find(|run| run.turn == turn.id)
                        .unwrap()
                        .status,
                    Status::Cancelled
                );
            } else {
                assert!(page.entries.iter().flat_map(|entry|&entry.parts).any(|part|matches!(part,Part::ToolResult {result,..} if result.to_string().contains("TimedOut"))));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert!(!fixture.root.join("late.txt").exists());
            assert!(!fixture.root.join("forbidden.txt").exists());
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn keeps_planning_turns_read_only() {
    for remote in [false, true] {
        let server = Server::tools(vec![(
            plugin_tool("files", "read_file"),
            json!({"path":"source.txt"}),
        )])
        .await;
        let mut fixture = Fixture::new(remote, &server).await;
        std::fs::write(fixture.root.join("source.txt"), "source").unwrap();
        ready(&mut fixture, WorkMode::Plan).await;
        save(
            &fixture.client,
            fixture.session.worktree,
            Settings {
                enabled: true,
                before_tool: script("printf before >> forbidden.txt"),
                after_tool: script("printf after >> forbidden.txt"),
                turn_end: script("printf end >> forbidden.txt"),
                ..Default::default()
            },
        )
        .await;
        let turn = approvals::rules::submit(&fixture.client, &fixture.session).await;
        let page = questions::finish_plan(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Completed
        );
        assert!(!fixture.root.join("forbidden.txt").exists());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
