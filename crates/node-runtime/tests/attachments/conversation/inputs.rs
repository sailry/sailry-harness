use super::*;
use serde_json::json;
#[cfg(unix)]
#[path = "inputs/lifecycle.rs"]
mod lifecycle;

async fn configure(
    client: &Client,
    session: Session,
    mut provider: Provider,
    permission: Permission,
) -> Session {
    provider.models[0].tools = true;
    provider.models[0].vision = false;
    client
        .execute(client.prepare(Command::PutProvider {
            provider,
            expected_revision: 1,
        }))
        .await
        .unwrap();
    let Output::Session(session) = client
        .execute(client.prepare(Command::SetSessionConfig {
            session: session.id,
            expected_revision: session.revision,
            config: SessionConfig {
                assistant: None,
                resource: None,
                permission,
                ..session.config
            },
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    session
}

#[cfg(unix)]
#[tokio::test]
async fn reads_copies_and_restores_receipts() {
    for remote in [false, true] {
        let mut fixture = Fixture::start().await;
        let client = fixture
            .clients()
            .into_iter()
            .nth(usize::from(remote))
            .unwrap();
        let bytes = b"opaque\0\xffarchive";
        let input_file = upload(
            &client,
            fixture.worktree,
            "包.zip",
            "application/zip",
            bytes,
        )
        .await;
        let unrelated = upload(
            &client,
            fixture.worktree,
            "unrelated.zip",
            "application/zip",
            bytes,
        )
        .await;
        std::fs::write(fixture.root.join("expected.bin"), bytes).unwrap();
        let command = format!(
            "cmp \"$SAILRY_ATTACHMENTS/{}\" expected.bin && printf '%s' \"$SAILRY_ATTACHMENTS\" > staged.txt && printf changed > \"$SAILRY_ATTACHMENTS/{}\" && printf x >> count.txt && printf read-ok",
            input_file.id, input_file.id
        );
        let calls = vec![
            (
                server::plugin_tool("commands", "run_command"),
                json!({"command": command, "attachments": [input_file.id]}),
            ),
            (
                server::plugin_tool("commands", "run_command"),
                json!({"command": "touch forbidden.txt", "attachments": [unrelated.id]}),
            ),
            (
                server::plugin_tool("commands", "run_command"),
                json!({"command": "touch duplicate.txt", "attachments": [input_file.id, input_file.id]}),
            ),
            (
                server::plugin_tool("commands", "run_command"),
                json!({"command": "test -z \"$SAILRY_ATTACHMENTS\" && printf isolated"}),
            ),
        ];
        let server = server::Server::tools(calls).await;
        let (session, provider) = session(
            &client,
            fixture.worktree,
            &server.endpoint,
            ModelApi::ChatCompletions,
        )
        .await;
        let session = configure(&client, session, provider, Permission::Full).await;
        let turn = queue(&client, &session, input("Inspect the file", &[&input_file])).await;
        let history = run(&client, &turn).await;
        assert_eq!(
            history.runs[0].status,
            Status::Completed,
            "{:?}",
            history.runs
        );
        let results: Vec<_> = history
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| {
                if let Part::ToolResult { result, .. } = part {
                    Some(result.clone())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(results.len(), 4);
        let Output::CommandResult(first) = serde_json::from_value(results[0].clone()).unwrap()
        else {
            panic!("command result expected")
        };
        assert_eq!(first.stdout.text, "read-ok");
        assert_eq!(first.outcome, sailry_protocol::process::Outcome::Exited(0));
        assert!(
            results[1].to_string().contains("not_found"),
            "{:?}",
            results[1]
        );
        assert!(
            results[2].to_string().contains("invalid_request"),
            "{:?}",
            results[2]
        );
        assert!(results[3].to_string().contains("isolated"));
        assert!(!fixture.root.join("forbidden.txt").exists());
        assert!(!fixture.root.join("duplicate.txt").exists());
        let staged = std::fs::read_to_string(fixture.root.join("staged.txt")).unwrap();
        assert!(!std::path::Path::new(&staged).exists());
        assert_eq!(download(&client, &input_file).await, bytes);
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 5);
        assert!(requests[0].to_string().contains("Attachment file:"));
        assert!(requests[0].to_string().contains("not included"));
        assert!(requests[0].to_string().contains(&input_file.id.to_string()));
        assert!(!requests[0].to_string().contains("sailry-attachment://"));
        assert!(
            !requests[0]
                .to_string()
                .contains(fixture.node.profile().to_str().unwrap())
        );

        let database =
            rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3"))
                .unwrap();
        let id: String = database
            .query_row(
                "SELECT request FROM agent_approvals WHERE turn=?1 AND call='call-fixture-0'",
                [turn.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        drop(database);
        let Output::Plugin(package) = client
            .execute(client.prepare(Command::ReadPlugin {
                name: "commands".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("package expected")
        };
        let request = Request {
            id: id.parse().unwrap(),
            ..client
                .prepare(Command::RunCommand {
                    turn: turn.id,
                    command,
                    cwd: String::new(),
                    timeout_ms: 120000,
                    background: false,
                    attachments: vec![input_file.id],
                })
                .with_plugin(plugin::Context {
                    invocation: None,
                    turn: Some(turn.id),
                    surface: Default::default(),
                    package: package.summary.reference(),
                    worktree: Some(session.worktree),
                    session: Some(session.id),
                })
        };
        assert_eq!(
            client.execute(request.clone()).await.unwrap(),
            Output::CommandResult(first.clone())
        );
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        fixture.node = Node::start(profile).await.unwrap();
        let client = fixture
            .clients()
            .into_iter()
            .nth(usize::from(remote))
            .unwrap();
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::CommandResult(first)
        );
        assert_eq!(std::fs::read(fixture.root.join("count.txt")).unwrap(), b"x");
        assert_eq!(page(&client, session.id).await, history);
        fixture.close().await;
    }
}

#[tokio::test]
async fn defers_native_support_checks() {
    let fixture = Fixture::start().await;
    let server = server::Server::start(false).await;
    for client in fixture.clients() {
        let (session, _) = session(
            &client,
            fixture.worktree,
            &server.endpoint,
            ModelApi::ChatCompletions,
        )
        .await;
        let file = upload(
            &client,
            fixture.worktree,
            "archive.zip",
            "application/zip",
            b"binary\0archive",
        )
        .await;
        let turn = queue(&client, &session, input("Inspect", &[&file])).await;
        let history = run(&client, &turn).await;
        assert_eq!(history.runs[0].status, Status::Failed);
        assert_eq!(
            history.runs[0].error.as_ref().unwrap().code,
            ErrorCode::InvalidRequest
        );
        assert!(
            history
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| *part == Part::Attachment(file.clone()))
        );
    }
    assert!(server.requests.lock().unwrap().is_empty());
    fixture.close().await;
}
