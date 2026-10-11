use super::*;
use std::sync::atomic::Ordering;

async fn enable_end(fixture: &Fixture) {
    save(
        &fixture.client,
        fixture.session.worktree,
        Settings {
            enabled: true,
            turn_end: script(
                "test \"$SAILRY_HOOK_STATUS\" = failed && printf end >> ends.txt; exit 9",
            ),
            ..Default::default()
        },
    )
    .await;
}

async fn submit(fixture: &Fixture) -> (Request, QueuedTurn) {
    let request = fixture.client.prepare(Command::SubmitTurn {
        session: fixture.session.id,
        expected_revision: fixture.session.revision,
        message: "Run the failure fixture".into(),
    });
    let Output::QueuedTurn(turn) = fixture.client.execute(request.clone()).await.unwrap() else {
        panic!("turn expected")
    };
    (request, turn)
}

fn failure(page: &Page, turn: TurnId, code: ErrorCode, original: &str) {
    let run = page.runs.iter().find(|run| run.turn == turn).unwrap();
    assert_eq!(run.status, Status::Failed, "{run:?}");
    let error = run.error.as_ref().unwrap();
    assert_eq!(error.code, code, "{error:?}");
    assert!(error.message.starts_with(original), "{error:?}");
    assert!(
        error.message.contains("turn_end hook failed: Exited(9)"),
        "{error:?}"
    );
}

#[tokio::test]
async fn retains_empty_completion_and_hook_failure() {
    for remote in [false, true] {
        let server = Server::empty_after_tool().await;
        let fixture = Fixture::new(remote, &server).await;
        let Output::Providers(providers) = execute(&fixture.client, Command::ListProviders).await
        else {
            panic!("providers expected")
        };
        let mut provider = providers
            .into_iter()
            .find(|provider| provider.id == fixture.session.config.provider)
            .unwrap();
        provider.api = ModelApi::Responses;
        execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await;
        std::fs::write(fixture.root.join("phase.txt"), "canonical file result").unwrap();
        enable_end(&fixture).await;

        let (request, turn) = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        failure(
            &page,
            turn.id,
            ErrorCode::Unavailable,
            "Model returned no answer",
        );
        let results = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| {
                if let Part::ToolResult { result, .. } = part {
                    Some(result)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(results.len(), 1);
        assert!(results[0].to_string().contains("canonical file result"));
        assert!(!results[0].to_string().contains("turn_end hook failed"));
        assert_eq!(
            fixture.client.execute(request).await.unwrap(),
            Output::QueuedTurn(turn)
        );
        assert_eq!(history(&fixture.client, fixture.session.id).await, page);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("ends.txt")).unwrap(),
            "end"
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn retains_uncertain_cleanup_and_hook_failure() {
    for remote in [false, true] {
        let peer = mcp::input::Server::task(
            json!({"type":"object", "properties":{"value":{"type":"string"}}, "required":["missing"]}),
            false,
        ).await;
        peer.tasks.ignore_cancel.store(true, Ordering::SeqCst);
        let server = Server::tools(vec![
            (
                plugin_tool("commands", "run_command"),
                json!({"command":"printf once >> actual.txt"}),
            ),
            (mcp::peer::alias("input", "read"), json!({})),
        ])
        .await;
        let mut fixture = Fixture::new(remote, &server).await;
        ready(&mut fixture, WorkMode::Code).await;
        peer.package(&fixture.root.join("package"));
        let Output::Plugin(_) = execute(
            &fixture.client,
            Command::InstallPlugin {
                worktree: fixture.session.worktree,
                path: "package".into(),
                name: "example".into(),
                expected_revision: 0,
                expected_digest: None,
            },
        )
        .await
        else {
            panic!("plugin expected")
        };
        enable_end(&fixture).await;

        let (request, turn) = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        failure(
            &page,
            turn.id,
            ErrorCode::OutcomeUnknown,
            "MCP task termination could not be confirmed",
        );
        let commands = process::results(&page);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].outcome, Outcome::Exited(0));
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("actual.txt")).unwrap(),
            "once"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("ends.txt")).unwrap(),
            "end"
        );
        assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
        assert_eq!(peer.tasks.cancelled.load(Ordering::SeqCst), 1);
        assert_eq!(peer.tasks.updates.load(Ordering::SeqCst), 0);
        assert!(page.questions.is_empty());
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| {
            matches!(part, Part::ToolResult {result, ..} if result["error"]["code"] == "outcome_unknown")
        }));
        assert_eq!(
            fixture.client.execute(request).await.unwrap(),
            Output::QueuedTurn(turn)
        );
        assert_eq!(history(&fixture.client, fixture.session.id).await, page);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
