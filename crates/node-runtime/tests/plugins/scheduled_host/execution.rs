use super::*;

async fn session(client: &Client, record: &Value) -> Session {
    let Output::Session(session) = execute(
        client,
        Command::ReadSession {
            session: serde_json::from_value(record["session"].clone()).unwrap(),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    session
}

async fn publish(client: &Client, context: &Context, task: &Value) -> Value {
    let jobs::Output::Published { jobs, .. } = dispatch::run(
        client,
        context,
        jobs::Command::Publish {
            topic: task["id"].as_str().unwrap().into(),
            payload: json!({"name":task["name"]}),
        },
    )
    .await
    else {
        panic!("published event expected")
    };
    assert_eq!(jobs.len(), 1);
    json!(jobs[0])
}

fn association(task: &Value) -> conversation::Association {
    conversation::Association {
        namespace: "scheduled-tasks".into(),
        kind: "task".into(),
        id: task["id"].as_str().unwrap().into(),
    }
}

#[tokio::test]
async fn captures_current_team_for_each_event() {
    for remote in [false, true] {
        let server = model_fixture::Server::markdown("Scheduled result".into()).await;
        let (directory, node, controller, client, tree) = fixture(remote).await;
        let context = installed(&client, directory.path(), tree).await;
        let configuration = config(&client, server.endpoint.clone()).await;
        let Output::Team(mut team) = execute(
            &client,
            Command::PutTeam {
                team: team::Profile {
                    id: TeamId::new(),
                    revision: 0,
                    key: "scheduled-review".into(),
                    name: "Scheduled reviewers".into(),
                    appearance: None,
                    description: String::new(),
                    model: None,
                    instructions: "First team instructions".into(),
                    members: vec![],
                },
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("team expected")
        };
        let mut input = draft(Some(configuration));
        input["execution"] = json!("team");
        input["team"] = json!({"id":team.id,"revision":team.revision.to_string()});
        let mut task = ok(&client, &context, "save", input).await;
        assert_eq!(task["team"], json!({"id":team.id,"revision":null}));
        let handlers = dispatch::run(&client, &context, jobs::Command::ListHandlers).await;
        let first = publish(&client, &context, &task).await;
        let record = wait(&client, &context, &first, "completed").await;
        let original = session(&client, &record).await;
        assert_eq!(original.roles.execution, conversation::Execution::Team);
        assert_eq!(original.roles.team, Some(team.reference()));
        assert_eq!(original.roles.instructions, team.instructions);
        assert_eq!(original.association, Some(association(&task)));

        team.instructions = "Updated team instructions".into();
        let Output::Team(updated) = execute(
            &client,
            Command::PutTeam {
                expected_revision: team.revision,
                team,
            },
        )
        .await
        else {
            panic!("team expected")
        };
        let second = publish(&client, &context, &task).await;
        let record = wait(&client, &context, &second, "completed").await;
        let current = session(&client, &record).await;
        assert_eq!(current.roles.team, Some(updated.reference()));
        assert_eq!(current.roles.instructions, updated.instructions);
        assert_eq!(current.association, original.association);
        assert_ne!(current.id, original.id);
        assert_eq!(
            dispatch::run(&client, &context, jobs::Command::ListHandlers).await,
            handlers
        );
        let Output::Session(retained) = execute(
            &client,
            Command::ReadSession {
                session: original.id,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        assert_eq!(retained.roles, original.roles);

        task["execution"] = json!("model");
        task["team"] = Value::Null;
        task = ok(&client, &context, "save", task).await;
        let job = ok(&client, &context, "run", json!({"id":task["id"]})).await;
        let record = wait(&client, &context, &job["id"], "completed").await;
        let direct = session(&client, &record).await;
        assert_eq!(direct.roles.execution, conversation::Execution::Model);
        assert!(direct.roles.team.is_none());
        assert!(direct.roles.profiles.is_empty());
        assert_eq!(direct.association, original.association);
        let Output::AssociatedSessions(related) = execute(
            &client,
            Command::ListAssociatedSessions {
                association: association(&task),
                before: None,
                limit: 10,
            },
        )
        .await
        else {
            panic!("associated sessions expected")
        };
        let ids: std::collections::BTreeSet<_> =
            related.sessions.iter().map(|session| session.id).collect();
        assert_eq!(ids, [original.id, current.id, direct.id].into());
        assert!(related.next_before.is_none());
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn manual_refresh_preserves_cursors_and_captured_jobs() {
    for remote in [false, true] {
        let start = Arc::new(Notify::new());
        let finish = Arc::new(Notify::new());
        let server = model_fixture::Server::held(start.clone(), finish.clone()).await;
        let (directory, node, controller, client, tree) = fixture(remote).await;
        let context = installed(&client, directory.path(), tree).await;
        let configuration = config(&client, server.endpoint.clone()).await;
        let mut input = draft(Some(configuration));
        input["execution"] = json!("model");
        input["enabled"] = json!(true);
        let task = ok(&client, &context, "save", input).await;
        let jobs::Output::Handlers(mut handlers) =
            dispatch::run(&client, &context, jobs::Command::ListHandlers).await
        else {
            panic!("handlers expected")
        };
        let mut handler = handlers.remove(0);
        let Command::StartSession(draft) = &mut *handler.callback.command else {
            panic!("session callback expected")
        };
        // Existing handlers may predate task associations; their admitted jobs
        // retain the original callback even when Run refreshes the definition.
        draft.association = None;
        draft.message = "Previously captured task".into();
        let jobs::Output::Handler(previous) =
            dispatch::run(&client, &context, jobs::Command::SaveHandler(handler)).await
        else {
            panic!("handler expected")
        };
        let schedules = dispatch::run(&client, &context, jobs::Command::ListSchedules).await;
        let stored = ok(&client, &context, "list", json!({})).await;
        let first = publish(&client, &context, &task).await;
        server.wait_count(1).await;
        let captured = wait(&client, &context, &first, "running").await;
        let original = session(&client, &captured).await;
        assert!(original.association.is_none());
        let receipt = dispatch::run(
            &client,
            &context,
            jobs::Command::ReadResult {
                id: serde_json::from_value(first.clone()).unwrap(),
            },
        )
        .await;
        let request = request(&client, &context, "run", json!({"id":task["id"]}));
        let output = client.execute(request.clone()).await.unwrap();
        assert_eq!(client.execute(request).await.unwrap(), output);
        let Output::PluginResult(result) = output else {
            panic!("callback result expected")
        };
        let second = result.get("Ok").unwrap_or_else(|| panic!("{result}"))["id"].clone();
        server.wait_count(2).await;
        let record = wait(&client, &context, &second, "running").await;
        let current = session(&client, &record).await;
        assert_eq!(current.association, Some(association(&task)));
        assert_ne!(current.id, original.id);
        assert_eq!(
            dispatch::run(&client, &context, jobs::Command::ListSchedules).await,
            schedules
        );
        assert_eq!(ok(&client, &context, "list", json!({})).await, stored);
        assert_eq!(wait(&client, &context, &first, "running").await, captured);
        assert_eq!(
            dispatch::run(
                &client,
                &context,
                jobs::Command::ReadResult {
                    id: serde_json::from_value(first.clone()).unwrap(),
                },
            )
            .await,
            receipt
        );
        let jobs::Output::Handlers(handlers) =
            dispatch::run(&client, &context, jobs::Command::ListHandlers).await
        else {
            panic!("handlers expected")
        };
        assert_eq!(handlers[0].revision, previous.revision + 1);
        assert!(
            matches!(&*handlers[0].callback.command, Command::StartSession(draft)
            if draft.association == Some(association(&task)))
        );
        start.notify_waiters();
        finish.notify_one();
        tokio::time::timeout(Duration::from_secs(15), async {
            while !history(&client, &context)
                .await
                .iter()
                .any(|record| record["job"]["status"] == "completed")
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        finish.notify_one();
        for id in [&first, &second] {
            wait(&client, &context, id, "completed").await;
        }
        let Output::Conversation(history) = execute(
            &client,
            Command::ReadConversation {
                session: original.id,
                before: None,
                limit: 10,
            },
        )
        .await
        else {
            panic!("conversation expected")
        };
        assert!(
            serde_json::to_string(&history)
                .unwrap()
                .contains("Previously captured task")
        );
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
