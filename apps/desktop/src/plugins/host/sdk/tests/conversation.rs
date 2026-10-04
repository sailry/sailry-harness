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
fn controls_captured_session_and_observes_shared_history() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let path = fixture.directory.path().join("project/package/plugin.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
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
        assert_eq!(session["id"], json!(fixture.session.id));
        assert_eq!(session["revision"], fixture.session.revision.to_string());
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
        assert_eq!(admitted["Ok"]["kind"], "queued_turn");
        assert_eq!(
            complete(&fixture, &module, "completeRequest", json!([request])),
            admitted
        );
        call(&module, "forgetRequest", json!([request]));
        let turn = admitted["Ok"]["data"]["id"].clone();
        let observed = until(&fixture, &module, |view| {
            view["snapshot"]["page"]["runs"]
                .as_array()
                .is_some_and(|runs| runs.len() == 1)
        });
        assert_eq!(
            observed["view"]["snapshot"]["page"]["session"],
            json!(fixture.session.id)
        );
        let mut config = fixture.session.config.clone();
        config.mode = WorkMode::Plan;
        let request = call(
            &module,
            "setSessionConfig",
            json!([config, session["revision"]]),
        );
        let saved = complete(&fixture, &module, "completeRequest", json!([request]));
        assert_eq!(saved["Ok"]["data"]["config"]["mode"], "plan");
        call(&module, "forgetRequest", json!([request]));
        let stale = call(
            &module,
            "sendMessage",
            json!([Input::from("Stale input"), session["revision"]]),
        );
        assert_eq!(
            complete(&fixture, &module, "completeRequest", json!([stale]))["Err"]["code"],
            "revision_conflict"
        );
        call(&module, "forgetRequest", json!([stale]));
        let stop = call(&module, "stopTurn", json!([turn]));
        let stopped = complete(&fixture, &module, "completeRequest", json!([stop]));
        assert_eq!(stopped["Ok"]["data"]["status"], "cancelled", "{stopped}");
        call(&module, "forgetRequest", json!([stop]));
        let stopped = until(&fixture, &module, |view| {
            view["snapshot"]["page"]["runs"][0]["status"] == "cancelled"
        });
        assert_eq!(
            stopped["view"]["snapshot"]["page"]["runs"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let history = complete(&fixture, &module, "readConversation", json!([]));
        assert_eq!(history["page"]["runs"].as_array().unwrap().len(), 1);
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
            "permission_denied"
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
            "wrong_target"
        );
        call(&module, "forgetRequest", json!([denied]));
        let Output::Conversation(other_history) = fixture.execute(Command::ReadConversation {
            session: other.id,
            before: None,
            limit: 10,
        }) else {
            panic!("conversation expected")
        };
        assert_eq!(
            other_history.page.runs[0].status,
            sailry_protocol::conversation::Status::Queued
        );
        // Pending subscriptions release with the captured view, without stopping Node tasks.
        let pending = module
            .begin("nextConversation", &args(json!([stopped["cursor"]])))
            .unwrap();
        host.close();
        assert!(fixture.runtime.block_on(pending).is_err());
        fixture.close();
    }
}
