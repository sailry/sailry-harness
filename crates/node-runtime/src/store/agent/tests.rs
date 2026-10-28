use super::*;

mod attention;
mod compaction;
mod continuation;
mod conversation_values;
pub(super) mod fixture;
mod forks;
mod hooks;
mod images;
mod inputs;
mod location;
mod preview;
mod records;
mod removal;
mod retry;
mod rewind;
mod title;

mod credentials {
    use super::*;

    #[test]
    fn binds_exact_replacement_without_copying_secrets() {
        let command = Command::SaveSsh {
            profile: ssh::Profile {
                id: SshId::new(),
                revision: 0,
                sharing: None,
                name: "Credential fixture".into(),
                host: "ssh.example.invalid".into(),
                port: 22,
                username: "fixture".into(),
                authentication: ssh::Authentication::Password,
                host_key: None,
            },
            expected_revision: 0,
            credential: Some(ssh::Credential::Password {
                password: Secret::new("prepared-private-marker".into()),
            }),
        };
        let prepared = approvals::Prepared::new(&command).unwrap().unwrap();
        assert!(prepared.matches(&command).unwrap());
        assert!(
            !serde_json::to_string(&prepared)
                .unwrap()
                .contains("prepared-private-marker")
        );
        let mut changed = command;
        let Command::SaveSsh { credential, .. } = &mut changed else {
            unreachable!()
        };
        *credential = Some(ssh::Credential::Password {
            password: Secret::new("replacement-private-marker".into()),
        });
        assert!(!prepared.matches(&changed).unwrap());
    }

    #[tokio::test]
    async fn projects_only_trusted_ssh_calls() {
        let mut fixture = fixture::Fixture::new().await;
        fixture::package(&fixture.database, fixture._root.path(), "ssh").await;
        let name = crate::plugins::tools::alias("ssh", "ssh_save_connection");
        let args = serde_json::json!({"name":"Host fixture","credential":{"kind":"password","password":"private-marker"},"key_path":"/private-key-marker"});
        let mut event = AdkEvent::new("credential-fixture");
        event.author = "assistant".into();
        event.set_content(adk_core::Content {
            role: "model".into(),
            parts: vec![
                adk_core::Part::FunctionCall {
                    id: Some("trusted".into()),
                    name: name.clone(),
                    args: args.clone(),
                    thought_signature: None,
                },
                adk_core::Part::FunctionCall {
                    id: Some("unknown".into()),
                    name: "user_literal".into(),
                    args: args.clone(),
                    thought_signature: None,
                },
                adk_core::Part::Text {
                    text: "User literal private-marker".into(),
                },
            ],
        });
        let turn = fixture.append([event.clone()]).await;
        let history = fixture.read(None, 100).await;
        let entry = history
            .page
            .entries
            .iter()
            .find(|entry| entry.id == event.id)
            .unwrap();
        for (index, expected) in [
            (0, serde_json::json!({"name":"Host fixture"})),
            (1, args.clone()),
        ] {
            let Part::ToolCall { id, summary, .. } = &entry.parts[index] else {
                panic!("canonical call expected")
            };
            let detail = history::tools::page(
                &fixture.database.connection,
                &fixture.database.tool_details,
                tool::detail::Reference {
                    session: history.page.session,
                    turn,
                    entry: entry.id.clone(),
                    index,
                    call: id.clone(),
                    revision: summary.revision.clone(),
                },
                None,
                tool::detail::MAX_DETAIL_PAGE_BYTES,
            )
            .await
            .unwrap();
            let detail: tool::detail::Detail = serde_json::from_str(&detail.text).unwrap();
            assert_eq!(detail.arguments, expected);
        }
        assert_eq!(
            entry.parts[2],
            Part::Text("User literal private-marker".into())
        );
        let body: Vec<u8> = fixture
            .database
            .connection
            .prepare("SELECT body FROM agent_events WHERE turn=?1 AND id=?2")
            .await
            .unwrap()
            .query_row(params![turn.to_string(), event.id.as_str()])
            .await
            .and_then(|row| row.get(0))
            .unwrap();
        let canonical: crate::agent::canonical::Record = serde_json::from_slice(&body).unwrap();
        assert!(
            matches!(&canonical.message().unwrap().content[0].kind, crate::agent::canonical::BlockKind::ToolRequest(request) if request.arguments == args)
        );
        assert!(
            history::project(&fixture.database.connection, 0, TurnId::new(), &canonical)
                .await
                .is_err()
        );
    }
}

pub(super) fn command<'a>(
    database: &'a mut Database,
    events: &'a broadcast::Sender<EventEnvelope>,
    command: Command,
) -> sailry_link::Pending<'a, Output> {
    Box::pin(async move {
        let (reply, response) = oneshot::channel();
        assert!(
            database
                .dispatch(
                    database.node,
                    Request::new(database.node, command),
                    reply,
                    events
                )
                .await
                .is_none()
        );
        response
            .await
            .unwrap()
            .unwrap()
            .completion
            .await
            .unwrap()
            .unwrap()
    })
}

#[tokio::test]
async fn rebuilds_from_durable_events() {
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().canonicalize().unwrap().join("node.turso");
    let node = NodeId([31; 32]);
    let mut database = Database::open(&path, node, None).await.unwrap();
    let (events, _) = broadcast::channel(64);
    let Output::Project(project) = command(
        &mut database,
        &events,
        Command::RegisterProject {
            name: "History fixture".into(),
            path: fixture.path().to_str().unwrap().into(),
        },
    )
    .await
    else {
        panic!("project expected")
    };
    let config = SessionConfig {
        features: Default::default(),
        assistant: None,
        resource: None,
        provider: ProviderId::new(),
        model: "fixture".into(),
        effort: Effort::Low,
        mode: sailry_protocol::WorkMode::Code,
        options: Default::default(),
        permission: sailry_protocol::Permission::Ask,
        credential: None,
    };
    let Output::Session(session) = command(
        &mut database,
        &events,
        Command::CreateSession {
            project: Some(project.id),
            worktree: None,
            config: Some(config),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    let Output::QueuedTurn(turn) = command(
        &mut database,
        &events,
        Command::QueueTurn {
            session: session.id,
            expected_revision: 1,
            message: "fixture prompt".into(),
        },
    )
    .await
    else {
        panic!("turn expected")
    };
    runs::start(&database.connection, turn.id).await.unwrap();
    assert_eq!(
        runs::claim(&mut database, &events)
            .await
            .unwrap()
            .unwrap()
            .turn
            .id,
        turn.id
    );
    let mut updates = events.subscribe();
    let mut event = AdkEvent::new("fixture-invocation");
    event.author = "assistant".into();
    event.set_content(adk_core::Content::new("model").with_text("persisted answer"));
    event.llm_request = Some("request copy".into());
    event.llm_response.error_message = Some("sensitive upstream diagnostic fixture".into());
    event.provider_metadata.insert(
        "gcp.vertex.agent.llm_response".into(),
        "response copy".into(),
    );
    event
        .provider_metadata
        .insert("gcp.vertex.agent.llm_request".into(), "request copy".into());
    event.actions.state_delta.extend([
        ("value".into(), serde_json::json!(1)),
        ("app:shared".into(), serde_json::json!(2)),
        ("user:shared".into(), serde_json::json!(3)),
        ("temp:ephemeral".into(), serde_json::json!(4)),
    ]);
    history::append(&mut database, session.id, turn.id, event.clone(), &events)
        .await
        .unwrap();
    updates.try_recv().unwrap();
    history::append(&mut database, session.id, turn.id, event.clone(), &events)
        .await
        .unwrap();
    assert!(updates.try_recv().is_err());
    let state = history::state(&database.connection, session.id)
        .await
        .unwrap();
    assert_eq!(state.len(), 3);
    assert_eq!(state["value"], 1);
    database
        .connection
        .execute("DELETE FROM agent_state", ())
        .await
        .unwrap();
    history::rebuild(&mut database.connection).await.unwrap();
    assert_eq!(
        history::state(&database.connection, session.id)
            .await
            .unwrap(),
        state
    );
    runs::finish(&mut database, turn.id, Status::Completed, None, &events)
        .await
        .unwrap();
    let original = history::pages::read(&database.connection, session.id, None, 100)
        .await
        .unwrap();
    assert_eq!(original.page.entries.len(), 1);
    event.set_content(adk_core::Content::new("model").with_text("different answer"));
    assert!(
        history::append(&mut database, session.id, turn.id, event.clone(), &events)
            .await
            .is_err()
    );
    event.id = "partial-fixture".into();
    event.llm_response.partial = true;
    assert!(
        history::append(&mut database, session.id, turn.id, event.clone(), &events)
            .await
            .is_err()
    );
    event.llm_response.partial = false;
    assert!(
        history::append(&mut database, SessionId::new(), turn.id, event, &events)
            .await
            .is_err()
    );
    assert_eq!(
        history::pages::read(&database.connection, session.id, None, 100)
            .await
            .unwrap(),
        original
    );
    let request = GetRequest {
        app_name: APP.into(),
        user_id: USER.into(),
        session_id: session.id.to_string(),
        num_recent_events: Some(1),
        after: None,
    };
    let recent = sessions::get(&database.connection, turn.id, request.clone())
        .await
        .unwrap();
    assert_eq!(recent.events().len(), 1);
    assert!(recent.events().at(0).unwrap().llm_request.is_none());
    assert_eq!(
        recent
            .events()
            .at(0)
            .unwrap()
            .llm_response
            .error_message
            .as_deref(),
        Some("model response reported an error")
    );
    assert!(
        !recent
            .events()
            .at(0)
            .unwrap()
            .provider_metadata
            .contains_key("gcp.vertex.agent.llm_response")
    );
    assert!(
        !recent
            .events()
            .at(0)
            .unwrap()
            .provider_metadata
            .contains_key("gcp.vertex.agent.llm_request")
    );
    assert_eq!(recent.state().all(), state);
    let empty = sessions::get(
        &database.connection,
        turn.id,
        GetRequest {
            num_recent_events: Some(0),
            ..request.clone()
        },
    )
    .await
    .unwrap();
    assert_eq!(empty.events().len(), 0);
    assert_eq!(empty.last_update_time(), recent.last_update_time());
    assert!(
        sessions::get(&database.connection, TurnId::new(), request.clone())
            .await
            .is_err()
    );
    assert!(
        sessions::get(
            &database.connection,
            turn.id,
            GetRequest {
                user_id: "another-owner".into(),
                ..request
            }
        )
        .await
        .is_err()
    );
    database.close().await.unwrap();
    let database = Database::open(&path, node, None).await.unwrap();
    assert_eq!(
        history::pages::read(&database.connection, session.id, None, 100)
            .await
            .unwrap(),
        original
    );
    assert_eq!(
        history::state(&database.connection, session.id)
            .await
            .unwrap(),
        state
    );
    database.close().await.unwrap();
}
