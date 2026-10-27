use super::*;
use sailry_protocol::{
    WorkMode,
    conversation::{ApprovalState, Part, Status},
};
mod fixture;
use fixture::{Editor, Fixture, models};

fn prompt(session: SessionId, request: RequestId, revision: u64, text: &str) -> Value {
    json!({"sessionId":session,"prompt":[{"type":"text","text":text}],
        "_meta":{"sailry":{"request_id":request,"expected_revision":revision}}})
}

fn answer(updates: &[Value]) -> String {
    updates
        .iter()
        .filter(|update| update["update"]["sessionUpdate"] == "agent_message_chunk")
        .filter_map(|update| update["update"]["content"]["text"].as_str())
        .collect()
}

#[tokio::test]
async fn detail_failure_keeps_observation_alive() {
    for remote in [false, true] {
        let model = models::Server::turn_tools(vec![(
            models::plugin_tool("files", "read_file"),
            json!({"path":"source.txt"}),
        )])
        .await;
        let fixture = Fixture::with_detail_loss(remote, model).await;
        std::fs::write(fixture.root().join("source.txt"), "Canonical source").unwrap();
        let mut editor = fixture.editor().await;
        editor.initialize().await;
        editor.load(&fixture).await;
        let completed = editor
            .request(
                3,
                "session/prompt",
                prompt(
                    fixture.session.id,
                    RequestId::new(),
                    fixture.session.revision,
                    "Read source.txt",
                ),
                None,
            )
            .await;
        assert!(completed.get("result").is_some(), "{completed}");
        assert!(matches!(
            fixture
                .loss
                .lock()
                .unwrap()
                .as_ref()
                .map(|request| &request.command),
            Some(Command::ReadToolDetail { .. })
        ));
        assert!(!answer(&editor.updates).is_empty());
        assert!(
            editor
                .updates
                .iter()
                .any(|update| update["update"]["sessionUpdate"] == "tool_call")
        );
        assert_eq!(fixture.page().await.runs[0].status, Status::Completed);
        editor.close().await;
        fixture.close().await;
    }
}

#[tokio::test]
async fn streams_canonical_history_and_retries() {
    for remote in [false, true] {
        let fixture = Fixture::new(
            remote,
            models::Server::markdown("ACP result 中文 🙂".into()).await,
            false,
        )
        .await;
        let mut editor = fixture.editor().await;
        let initialized = editor.initialize().await;
        assert_eq!(initialized["result"]["protocolVersion"], 1);
        assert_eq!(
            initialized["result"]["agentCapabilities"]["loadSession"],
            true
        );
        assert_eq!(
            initialized["result"]["agentCapabilities"]["sessionCapabilities"]["fork"],
            json!({})
        );
        assert!(editor.load(&fixture).await.get("result").is_some());
        let request = RequestId::new();
        let input = prompt(
            fixture.session.id,
            request,
            fixture.session.revision,
            "Complete the editor task",
        );
        let completed = editor
            .request(3, "session/prompt", input.clone(), None)
            .await;
        assert_eq!(completed["result"]["stopReason"], "end_turn");
        assert_eq!(
            completed["result"]["_meta"]["sailry"]["request_id"],
            json!(request)
        );
        assert_eq!(
            completed["result"]["_meta"]["sailry"]["revision"],
            fixture.session.revision
        );
        assert_eq!(answer(&editor.updates), "ACP result 中文 🙂");
        let page = fixture.page().await;
        let turn = page.runs[0].turn;
        assert_eq!(completed["result"]["_meta"]["sailry"]["turn"], json!(turn));
        assert_eq!(page.runs[0].status, Status::Completed);
        assert_eq!(
            page.entries
                .iter()
                .filter(|entry| entry.author != "user")
                .flat_map(|entry| &entry.parts)
                .filter_map(|part| match part {
                    Part::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<String>(),
            answer(&editor.updates)
        );
        assert!(
            editor
                .request(4, "session/prompt", input, None)
                .await
                .get("result")
                .is_some()
        );
        let rejected = editor
            .request(
                5,
                "session/prompt",
                prompt(
                    fixture.session.id,
                    request,
                    fixture.session.revision,
                    "Changed payload",
                ),
                None,
            )
            .await;
        assert_eq!(
            rejected["error"]["data"]["sailry"]["error"]["code"],
            "conflict"
        );
        assert_eq!(fixture.page().await.runs.len(), 1);
        assert_eq!(fixture.model.requests.lock().unwrap().len(), 1);
        editor.close().await;
        fixture.close().await;
    }
}

#[tokio::test]
async fn preserves_uncertain_admission_identity() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote, models::Server::start(false).await, true).await;
        let mut editor = fixture.editor().await;
        editor.initialize().await;
        editor.load(&fixture).await;
        let request = RequestId::new();
        let input = prompt(
            fixture.session.id,
            request,
            fixture.session.revision,
            "Admit once",
        );
        let uncertain = editor
            .request(3, "session/prompt", input.clone(), None)
            .await;
        assert_eq!(uncertain["error"]["data"]["sailry"]["state"], "uncertain");
        assert_eq!(
            uncertain["error"]["data"]["sailry"]["request_id"],
            json!(request)
        );
        assert_eq!(
            uncertain["error"]["data"]["sailry"]["session"],
            json!(fixture.session.id)
        );
        assert_eq!(fixture.page().await.runs.len(), 1);
        assert!(
            editor
                .request(4, "session/prompt", input, None)
                .await
                .get("result")
                .is_some()
        );
        assert_eq!(fixture.page().await.runs.len(), 1);
        assert_eq!(fixture.model.requests.lock().unwrap().len(), 1);
        editor.close().await;
        fixture.close().await;
    }
}

#[tokio::test]
async fn maps_permission_to_captured_node_call() {
    for remote in [false, true] {
        for choice in ["allow", "deny"] {
            let model = models::Server::turn_tools(vec![(
                models::plugin_tool("files", "write_file"),
                json!({"path":"approved.txt","text":"Editor decision","expected_revision":null}),
            )])
            .await;
            let fixture = Fixture::new(remote, model, false).await;
            let mut editor = fixture.editor().await;
            editor.initialize().await;
            editor.load(&fixture).await;
            let completed = editor
                .request(
                    3,
                    "session/prompt",
                    prompt(
                        fixture.session.id,
                        RequestId::new(),
                        fixture.session.revision,
                        "Write approved.txt",
                    ),
                    Some(choice),
                )
                .await;
            assert!(completed.get("result").is_some(), "{completed}");
            assert_eq!(editor.permissions.len(), 1);
            let captured = &editor.permissions[0]["_meta"]["sailry"]["approval"];
            assert_eq!(captured["state"], "pending");
            assert_eq!(captured["session"], json!(fixture.session.id));
            assert_eq!(
                editor.permissions[0]["options"].as_array().unwrap().len(),
                2
            );
            assert!(
                editor.permissions[0]["options"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|option| matches!(
                        option["kind"].as_str(),
                        Some("allow_once" | "reject_once")
                    ))
            );
            let page = fixture.page().await;
            assert_eq!(
                page.approvals[0].state,
                if choice == "allow" {
                    ApprovalState::Approved
                } else {
                    ApprovalState::Denied
                }
            );
            assert_eq!(json!(page.approvals[0].id), captured["id"]);
            assert_eq!(
                fixture.root().join("approved.txt").exists(),
                choice == "allow"
            );
            assert!(
                editor
                    .updates
                    .iter()
                    .any(|update| update["update"]["sessionUpdate"] == "tool_call")
            );
            assert!(
                editor
                    .updates
                    .iter()
                    .any(
                        |update| update["update"]["sessionUpdate"] == "tool_call_update"
                            && update["update"].get("rawOutput").is_some()
                    )
            );
            editor.close().await;
            fixture.close().await;
        }
    }
}

#[tokio::test]
async fn retains_node_ownership_on_disconnect_and_cancel() {
    for remote in [false, true] {
        for explicit in [false, true] {
            let release = CancellationToken::new();
            let fixture = Fixture::new(
                remote,
                models::Server::markdown_held("Still running".into(), release.clone()).await,
                false,
            )
            .await;
            let mut editor = fixture.editor().await;
            editor.initialize().await;
            editor.load(&fixture).await;
            editor.send(json!({"jsonrpc":"2.0","id":3,"method":"session/prompt", "params":prompt(fixture.session.id, RequestId::new(), fixture.session.revision, "Wait for release")}));
            admitted(&mut editor).await;
            assert_eq!(fixture.page().await.runs[0].status, Status::Running);
            if explicit {
                editor.send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":fixture.session.id}}));
                let cancelled = editor.reply(3, None).await;
                assert_eq!(
                    cancelled["result"]["stopReason"], "cancelled",
                    "{cancelled}"
                );
                assert_eq!(fixture.settled().await.runs[0].status, Status::Cancelled);
                editor.close().await;
            } else {
                editor.close().await;
                fixture.node.observe().probe().await.unwrap();
                assert_eq!(fixture.page().await.runs[0].status, Status::Running);
                release.cancel();
                assert_eq!(fixture.settled().await.runs[0].status, Status::Completed);
            }
            fixture.close().await;
        }
    }
}

#[tokio::test]
async fn rpc_cancellation_only_stops_observation() {
    for remote in [false, true] {
        let release = CancellationToken::new();
        let fixture = Fixture::new(
            remote,
            models::Server::markdown_held("Survives cancellation".into(), release.clone()).await,
            false,
        )
        .await;
        let mut editor = fixture.editor().await;
        editor.initialize().await;
        editor.load(&fixture).await;
        editor.send(json!({"jsonrpc":"2.0","id":3,"method":"session/prompt", "params":prompt(fixture.session.id, RequestId::new(), fixture.session.revision, "Keep working")}));
        admitted(&mut editor).await;
        editor.send(json!({"jsonrpc":"2.0","method":"$/cancel_request","params":{"requestId":3}}));
        let response = editor.reply(3, None).await;
        assert_eq!(
            response["error"]["data"]["sailry"]["state"],
            "observation_cancelled"
        );
        assert_eq!(fixture.page().await.runs[0].status, Status::Running);
        release.cancel();
        assert_eq!(fixture.settled().await.runs[0].status, Status::Completed);
        editor.close().await;
        fixture.close().await;
    }
}

#[tokio::test]
async fn explicit_session_close_stops_ongoing_work() {
    for remote in [false, true] {
        let fixture = Fixture::new(
            remote,
            models::Server::markdown_held("Held answer".into(), CancellationToken::new()).await,
            false,
        )
        .await;
        let mut editor = fixture.editor().await;
        editor.initialize().await;
        editor.load(&fixture).await;
        editor.send(json!({"jsonrpc":"2.0","id":3,"method":"session/prompt", "params":prompt(fixture.session.id,
            RequestId::new(), fixture.session.revision, "Stop only on an explicit protocol request")}));
        admitted(&mut editor).await;
        let closed = editor
            .request(
                4,
                "session/close",
                json!({"sessionId":fixture.session.id}),
                None,
            )
            .await;
        assert!(closed.get("result").is_some(), "{closed}");
        assert_eq!(fixture.settled().await.runs[0].status, Status::Cancelled);
        let Output::Session(session) = fixture
            .execute(Command::ReadSession {
                session: fixture.session.id,
            })
            .await
        else {
            panic!("session expected")
        };
        assert_eq!(session.id, fixture.session.id);
        assert_eq!(session.config, fixture.session.config);
        assert!(!session.archived);
        editor.close().await;
        fixture.close().await;
    }
}

async fn admitted(editor: &mut Editor) -> Value {
    loop {
        let frame = editor.next().await;
        assert_eq!(frame["method"], "session/update", "{frame}");
        let update = frame["params"].clone();
        let admitted = update["_meta"]["sailry"]["state"] == "admitted";
        editor.updates.push(update.clone());
        if admitted {
            return update;
        }
    }
}

mod delegation;
mod sessions;
