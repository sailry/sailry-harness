use super::*;
use sailry_protocol::{WorkMode, conversation::Input};

fn until(fixture: &Fixture, module: &HostModule, predicate: impl Fn(&Value) -> bool) -> Value {
    fixture.runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(10), async {
            let mut cursor = String::new();
            loop {
                let value = decode(
                    &module
                        .begin("nextConversation", &args(json!([cursor])))
                        .unwrap()
                        .await
                        .unwrap(),
                )
                .unwrap();
                if predicate(&value["view"]) {
                    return value;
                }
                cursor = value["cursor"].as_str().unwrap().into();
            }
        })
        .await
        .expect("conversation subscription deadline")
    })
}

#[test]
fn captured_sessions() {
    for (scenario, check) in [
        (
            "controls_captured_session_and_observes_shared_history",
            (|_scenario: &str| {
                for remote in [false, true] {
                    let fixture = Fixture::new(remote);
                    fixture.package();
                    let path = fixture.directory.path().join("project/package/plugin.json");
                    let mut manifest: Value =
                        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                    manifest["extensions"]["dev.sailry.platform"]["actions"] =
                        json!(["conversation.read", "conversation.control"]);
                    std::fs::write(path, manifest.to_string()).unwrap();
                    let info = fixture.install(0);
                    let host = Host::new(
                        fixture.binding.client.clone(),
                        Context {
                            invocation: None,
                            turn: None,
                            surface: Default::default(),
                            package: info.summary.reference(),
                            worktree: Some(fixture.session.worktree),
                            session: Some(fixture.session.id),
                        },
                        fixture.runtime.handle().clone(),
                        false,
                        None,
                    );
                    let module = host.sdk();
                    let session = complete(&fixture, &module, "readSession", json!([]));
                    assert_eq!(
                        session["id"],
                        json!(fixture.session.id),
                        "controls captured session and observes shared history"
                    );
                    assert_eq!(
                        session["revision"],
                        fixture.session.revision.to_string(),
                        "controls captured session and observes shared history"
                    );
                    fixture.execute(Command::SetQueuePaused {
                        session: fixture.session.id,
                        expected_revision: 0,
                        paused: true,
                    });
                    let request = call(
                        &module,
                        "sendMessage",
                        json!([Input::from("Review this project"), session["revision"]]),
                    );
                    let admitted = complete(&fixture, &module, "completeRequest", json!([request]));
                    assert_eq!(
                        admitted["Ok"]["kind"], "queued_turn",
                        "controls captured session and observes shared history"
                    );
                    assert_eq!(
                        complete(&fixture, &module, "completeRequest", json!([request])),
                        admitted,
                        "controls captured session and observes shared history"
                    );
                    call(&module, "forgetRequest", json!([request]));
                    let turn = admitted["Ok"]["data"]["id"].clone();
                    let metadata =
                        complete(&fixture, &module, "readConversationRuns", json!([null, 1]));
                    assert_eq!(
                        metadata["session"],
                        json!(fixture.session.id),
                        "controls captured session and observes shared history"
                    );
                    assert!(
                        metadata["revision"].is_string(),
                        "controls captured session and observes shared history"
                    );
                    assert!(
                        metadata["queue_revision"].is_string(),
                        "controls captured session and observes shared history"
                    );
                    assert_eq!(
                        metadata["runs"][0]["turn"], turn,
                        "controls captured session and observes shared history"
                    );
                    assert!(
                        metadata["runs"][0].get("input").is_none(),
                        "controls captured session and observes shared history"
                    );
                    assert_eq!(
                        complete(
                            &fixture,
                            &module,
                            "readConversationRun",
                            json!([turn, metadata["revision"]])
                        ),
                        metadata["runs"][0],
                        "controls captured session and observes shared history"
                    );
                    let observed = until(&fixture, &module, |view| {
                        view["snapshot"]["page"]["runs"]
                            .as_array()
                            .is_some_and(|runs| runs.len() == 1)
                    });
                    assert_eq!(
                        observed["view"]["snapshot"]["page"]["session"],
                        json!(fixture.session.id),
                        "controls captured session and observes shared history"
                    );
                    let mut config = fixture.session.config.clone();
                    config.mode = WorkMode::Plan;
                    let request = call(
                        &module,
                        "setSessionConfig",
                        json!([config, session["revision"]]),
                    );
                    let saved = complete(&fixture, &module, "completeRequest", json!([request]));
                    assert_eq!(
                        saved["Ok"]["data"]["config"]["mode"], "plan",
                        "controls captured session and observes shared history"
                    );
                    call(&module, "forgetRequest", json!([request]));
                    let stale = call(
                        &module,
                        "sendMessage",
                        json!([Input::from("Stale input"), session["revision"]]),
                    );
                    assert_eq!(
                        complete(&fixture, &module, "completeRequest", json!([stale]))["Err"]["code"],
                        "revision_conflict",
                        "controls captured session and observes shared history"
                    );
                    call(&module, "forgetRequest", json!([stale]));
                    let before = until(&fixture, &module, |view| {
                        view["snapshot"]["page"]["runs"][0]["status"] == "queued"
                    });
                    let first = module
                        .begin("nextConversation", &args(json!([before["cursor"]])))
                        .unwrap();
                    let second = module
                        .begin("nextConversation", &args(json!([before["cursor"]])))
                        .unwrap();
                    let stop = call(&module, "stopTurn", json!([turn]));
                    let stopped = complete(&fixture, &module, "completeRequest", json!([stop]));
                    assert_eq!(
                        stopped["Ok"]["data"]["status"], "cancelled",
                        "controls captured session and observes shared history: {stopped}"
                    );
                    call(&module, "forgetRequest", json!([stop]));
                    fixture.runtime.block_on(async {
                        let (first, second) = tokio::time::timeout(
                            Duration::from_secs(10),
                            futures::future::join(first, second),
                        )
                        .await
                        .expect("concurrent conversation subscription deadline");
                        let first = decode(&first.unwrap()).unwrap();
                        let second = decode(&second.unwrap()).unwrap();
                        assert_ne!(
                            first["cursor"], before["cursor"],
                            "controls captured session and observes shared history"
                        );
                        assert_ne!(
                            second["cursor"], before["cursor"],
                            "controls captured session and observes shared history"
                        );
                    });
                    let stopped = until(&fixture, &module, |view| {
                        view["snapshot"]["page"]["runs"][0]["status"] == "cancelled"
                    });
                    assert_eq!(
                        stopped["view"]["snapshot"]["page"]["runs"]
                            .as_array()
                            .unwrap()
                            .len(),
                        1,
                        "controls captured session and observes shared history"
                    );
                    let history = complete(&fixture, &module, "readConversation", json!([]));
                    assert_eq!(
                        history["page"]["runs"].as_array().unwrap().len(),
                        1,
                        "controls captured session and observes shared history"
                    );
                    let Output::Session(other) = fixture.execute(Command::CreateSession {
                        project: fixture.session.project,
                        worktree: Some(fixture.session.worktree),
                        config: Some(config),
                    }) else {
                        panic!("session expected")
                    };
                    let denied = call(
                        &module,
                        "prepareRequest",
                        json!([Command::SetSessionConfig {
                            session: other.id,
                            expected_revision: other.revision,
                            config: other.config.clone(),
                        }]),
                    );
                    assert_eq!(
                        complete(&fixture, &module, "completeRequest", json!([denied]))["Err"]["code"],
                        "permission_denied",
                        "controls captured session and observes shared history"
                    );
                    call(&module, "forgetRequest", json!([denied]));
                    fixture.execute(Command::SetQueuePaused {
                        session: other.id,
                        expected_revision: 0,
                        paused: true,
                    });
                    let Output::QueuedTurn(other_turn) = fixture.execute(Command::QueueTurn {
                        session: other.id,
                        expected_revision: other.revision,
                        message: Input::from("Other session"),
                    }) else {
                        panic!("queued turn expected")
                    };
                    let denied = call(&module, "stopTurn", json!([other_turn.id]));
                    assert_eq!(
                        complete(&fixture, &module, "completeRequest", json!([denied]))["Err"]["code"],
                        "wrong_target",
                        "controls captured session and observes shared history"
                    );
                    call(&module, "forgetRequest", json!([denied]));
                    let Output::Conversation(other_history) =
                        fixture.execute(Command::ReadConversation {
                            session: other.id,
                            before: None,
                            limit: 10,
                        })
                    else {
                        panic!("conversation expected")
                    };
                    assert_eq!(
                        other_history.page.runs[0].status,
                        sailry_protocol::conversation::Status::Queued,
                        "controls captured session and observes shared history"
                    );
                    // Pending subscriptions release with the captured view, without stopping Node tasks.
                    let pending = module
                        .begin("nextConversation", &args(json!([stopped["cursor"]])))
                        .unwrap();
                    host.close();
                    assert!(
                        fixture.runtime.block_on(pending).is_err(),
                        "controls captured session and observes shared history"
                    );
                    fixture.close();
                }
            }) as fn(&str),
        ),
        (
            "run_projection_skips_content_permission_reads",
            (|_scenario: &str| {
                use sailry_protocol::conversation::{Draft, Part, Status};
                use sailry_protocol::{ErrorCode, Fault};
                use std::sync::Arc;

                for remote in [false, true] {
                    let fixture = Fixture::new(remote);
                    fixture.package();
                    let path = fixture.directory.path().join("project/package/plugin.json");
                    let mut manifest: Value =
                        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                    manifest["extensions"]["dev.sailry.platform"]["actions"] =
                        json!(["conversation.read"]);
                    std::fs::write(path, manifest.to_string()).unwrap();
                    let info = fixture.install(0);
                    let context = Context {
                        invocation: None,
                        turn: None,
                        surface: Default::default(),
                        package: info.summary.reference(),
                        worktree: Some(fixture.session.worktree),
                        session: Some(fixture.session.id),
                    };
                    let live = Host::new(
                        fixture.binding.client.clone(),
                        context.clone(),
                        fixture.runtime.handle().clone(),
                        false,
                        None,
                    );
                    fixture.execute(Command::SetQueuePaused {
                        session: fixture.session.id,
                        expected_revision: 0,
                        paused: true,
                    });
                    fixture.execute(Command::QueueTurn {
                        session: fixture.session.id,
                        expected_revision: fixture.session.revision,
                        message: Input::from("Keep this canonical input"),
                    });
                    until(&fixture, &live.sdk(), |view| {
                        view["snapshot"]["page"]["runs"]
                            .as_array()
                            .is_some_and(|runs| runs.len() == 1)
                    });
                    let view = live.conversation.receiver.get().unwrap().borrow().clone();
                    let turn = view.snapshot.as_ref().unwrap().page.runs[0].turn;
                    live.close();
                    let Output::Conversation(before) = fixture.execute(Command::ReadConversation {
                        session: fixture.session.id,
                        before: None,
                        limit: 20,
                    }) else {
                        panic!("conversation history expected")
                    };
                    let host = Host::new(
                        fixture.binding.client.clone(),
                        context,
                        fixture.runtime.handle().clone(),
                        false,
                        None,
                    );
                    let (sender, receiver) = tokio::sync::watch::channel(view);
                    host.conversation.receiver.set(receiver).ok().unwrap();
                    let module = host.sdk();
                    let first =
                        complete(&fixture, &module, "nextConversation", json!(["", "runs"]));
                    assert!(
                        first["view"]["snapshot"]["drafts"].is_null(),
                        "run projection skips content permission reads"
                    );
                    let reads = || {
                        fixture
                            .transport
                            .requests
                            .lock()
                            .unwrap()
                            .iter()
                            .filter(|request| {
                                matches!(request.command, Command::ReadSession { .. })
                            })
                            .count()
                    };
                    let count = reads();
                    let mut pending = module
                        .begin("nextConversation", &args(json!([first["cursor"], "runs"])))
                        .unwrap();
                    fixture.runtime.block_on(async {
                        assert!(
                            tokio::time::timeout(Duration::from_millis(40), &mut pending)
                                .await
                                .is_err(),
                            "run projection skips content permission reads"
                        );
                        assert_eq!(
                            reads(),
                            count + 1,
                            "run projection skips content permission reads"
                        );
                        for index in 0..64 {
                            sender.send_modify(|view| {
                                let snapshot = Arc::make_mut(view.snapshot.as_mut().unwrap());
                                snapshot.sequence += 1;
                                snapshot.drafts = vec![Draft {
                                    id: "stream".into(),
                                    turn,
                                    author: "model".into(),
                                    branch: String::new(),
                                    parts: vec![Part::Text(format!("delta {index}"))],
                                }];
                            });
                            assert!(
                                tokio::time::timeout(Duration::from_millis(1), &mut pending)
                                    .await
                                    .is_err(),
                                "run projection skips content permission reads"
                            );
                        }
                        assert!(
                            tokio::time::timeout(Duration::from_millis(40), &mut pending)
                                .await
                                .is_err(),
                            "run projection skips content permission reads"
                        );
                        assert_eq!(
                            reads(),
                            count + 1,
                            "run projection skips content permission reads"
                        );
                        sender.send_modify(|view| {
                            let snapshot = Arc::make_mut(view.snapshot.as_mut().unwrap());
                            Arc::make_mut(&mut snapshot.page).runs[0].status = Status::Stopping;
                        });
                        let changed = decode(
                            &tokio::time::timeout(Duration::from_secs(10), pending)
                                .await
                                .unwrap()
                                .unwrap(),
                        )
                        .unwrap();
                        assert_eq!(
                            changed["view"]["snapshot"]["page"]["runs"][0]["status"], "stopping",
                            "run projection skips content permission reads"
                        );
                        assert_eq!(
                            reads(),
                            count + 2,
                            "run projection skips content permission reads"
                        );
                    });
                    let full = complete(&fixture, &module, "nextConversation", json!([]));
                    assert_eq!(
                        full["view"]["snapshot"]["drafts"][0]["parts"][0]["data"], "delta 63",
                        "run projection skips content permission reads"
                    );
                    for (status, code) in [
                        (Status::Interrupted, Some(ErrorCode::OutcomeUnknown)),
                        (Status::Cancelled, None),
                    ] {
                        sender.send_modify(|view| {
                            let snapshot = Arc::make_mut(view.snapshot.as_mut().unwrap());
                            let run = &mut Arc::make_mut(&mut snapshot.page).runs[0];
                            run.status = status;
                            run.error = code.map(|code| Fault::new(code, "fixture outcome"));
                        });
                        let value =
                            complete(&fixture, &module, "nextConversation", json!(["", "runs"]));
                        assert_eq!(
                            value["view"]["snapshot"]["page"]["runs"][0]["status"],
                            json!(status),
                            "run projection skips content permission reads"
                        );
                        assert_eq!(
                            value["view"]["snapshot"]["page"]["runs"][0]["error"]["code"],
                            json!(code),
                            "run projection skips content permission reads"
                        );
                    }
                    assert!(
                        module
                            .begin("nextConversation", &args(json!(["", "tokens"])))
                            .is_err(),
                        "run projection skips content permission reads"
                    );
                    let Output::Conversation(after) = fixture.execute(Command::ReadConversation {
                        session: fixture.session.id,
                        before: None,
                        limit: 20,
                    }) else {
                        panic!("conversation history expected")
                    };
                    // Stream watermarks belong to subscription generations, not stored history.
                    assert_eq!(
                        before.page, after.page,
                        "run projection skips content permission reads"
                    );
                    assert_eq!(
                        before.missing, after.missing,
                        "run projection skips content permission reads"
                    );
                    let current =
                        complete(&fixture, &module, "nextConversation", json!(["", "runs"]));
                    let mut pending = module
                        .begin(
                            "nextConversation",
                            &args(json!([current["cursor"], "runs"])),
                        )
                        .unwrap();
                    fixture.runtime.block_on(async {
                        assert!(
                            tokio::time::timeout(Duration::from_millis(40), &mut pending)
                                .await
                                .is_err(),
                            "run projection skips content permission reads"
                        );
                    });
                    fixture.execute(Command::SetPluginEnabled {
                        name: info.summary.name,
                        expected_revision: info.summary.revision,
                        enabled: false,
                    });
                    sender.send_modify(|view| {
                        let snapshot = Arc::make_mut(view.snapshot.as_mut().unwrap());
                        Arc::make_mut(&mut snapshot.page).runs[0].status = Status::Failed;
                    });
                    assert!(
                        fixture
                            .runtime
                            .block_on(async {
                                tokio::time::timeout(Duration::from_secs(10), pending)
                                    .await
                                    .unwrap()
                            })
                            .is_err(),
                        "run projection skips content permission reads"
                    );
                    host.close();
                    fixture.close();
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
