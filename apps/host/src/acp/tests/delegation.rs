use super::*;
use sailry_protocol::{RoleId, role};

#[tokio::test]
async fn resolves_child_approval_with_native_identity() {
    for remote in [false, true] {
        let parent = models::Server::turn_tools(vec![(
            models::plugin_tool("delegation", "spawn_agent"),
            json!({"role":"writer","task":"Write child.txt with the files tool"}),
        )])
        .await;
        let child = models::Server::turn_tools(vec![(
            models::plugin_tool("files", "write_file"),
            json!({"path":"child.txt","text":"Delegated editor task","expected_revision":null}),
        )])
        .await;
        let mut fixture = Fixture::new(remote, parent, false).await;
        writer(&mut fixture, &child).await;
        let mut editor = fixture.editor().await;
        editor.initialize().await;
        editor.load(&fixture).await;
        let result = editor
            .request(
                3,
                "session/prompt",
                prompt(
                    fixture.session.id,
                    RequestId::new(),
                    fixture.session.revision,
                    "Delegate the fixture file",
                ),
                Some("allow"),
            )
            .await;
        assert!(result.get("result").is_some(), "{result}");
        let decision = editor
            .permissions
            .iter()
            .find(|permission| {
                permission["_meta"]["sailry"]["session"] != json!(fixture.session.id)
            })
            .unwrap();
        let child_session: SessionId =
            serde_json::from_value(decision["_meta"]["sailry"]["session"].clone()).unwrap();
        assert_eq!(decision["sessionId"], json!(fixture.session.id));
        assert_ne!(child_session, fixture.session.id);
        let history = fixture
            .client
            .read_conversation(child_session, None, 100)
            .await
            .unwrap();
        assert_eq!(history.page.approvals.len(), 1);
        assert_eq!(history.page.approvals[0].state, ApprovalState::Approved);
        assert_eq!(
            json!(history.page.approvals[0].id),
            decision["_meta"]["sailry"]["approval"]["id"]
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root().join("child.txt")).unwrap(),
            "Delegated editor task"
        );
        assert_eq!(fixture.page().await.runs[0].status, Status::Completed);
        assert!(
            editor
                .updates
                .iter()
                .any(
                    |update| update["_meta"]["sailry"]["session"] == json!(child_session)
                        && update["update"]["sessionUpdate"] == "tool_call_update"
                )
        );
        editor.close().await;
        fixture.close().await;
    }
}

#[tokio::test]
async fn recovers_after_decision_loss() {
    for remote in [false, true] {
        for delegated in [false, true] {
            let model = models::Server::scripted_tools(vec![
                (models::plugin_tool("files", "write_file"),
                    json!({"path":"approved.txt","text":"Admitted decision","expected_revision":null})),
                (models::plugin_tool("files", "write_file"),
                    json!({"path":"pending.txt","text":"Later decision","expected_revision":null})),
            ]).await;
            let (parent, child) = if delegated {
                (
                    models::Server::turn_tools(vec![(
                        models::plugin_tool("delegation", "spawn_agent"),
                        json!({"role":"writer","task":"Write both fixture files"}),
                    )])
                    .await,
                    Some(model),
                )
            } else {
                (model, None)
            };
            let mut fixture = Fixture::with_decision_loss(remote, parent).await;
            if let Some(child) = &child {
                writer(&mut fixture, child).await;
            }
            let mut editor = fixture.editor().await;
            editor.initialize().await;
            editor.load(&fixture).await;
            editor.send(json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":prompt(
                fixture.session.id, RequestId::new(), fixture.session.revision, "Write the fixture files")}));
            let permission = loop {
                let frame = editor.next().await;
                if frame["method"] == "session/request_permission" {
                    editor.permissions.push(frame["params"].clone());
                    break frame;
                }
                assert_eq!(frame["method"], "session/update", "{frame}");
                editor.updates.push(frame["params"].clone());
            };
            editor.send(json!({"jsonrpc":"2.0","id":permission["id"],
                "result":{"outcome":{"outcome":"selected","optionId":"allow"}}}));
            let uncertain = editor.reply(3, None).await;
            let source: SessionId =
                serde_json::from_value(permission["params"]["_meta"]["sailry"]["session"].clone())
                    .unwrap();
            let decision: RequestId = serde_json::from_value(
                permission["params"]["_meta"]["sailry"]["decision_request_id"].clone(),
            )
            .unwrap();
            assert_eq!(
                uncertain["error"]["data"]["sailry"]["state"], "uncertain",
                "{uncertain}"
            );
            assert_eq!(
                uncertain["error"]["data"]["sailry"]["operation"]["request_id"],
                json!(decision)
            );
            assert_eq!(
                uncertain["error"]["data"]["sailry"]["operation"]["session"],
                json!(source)
            );
            assert_eq!(
                uncertain["error"]["data"]["sailry"]["operation"]["root_turn"],
                uncertain["error"]["data"]["sailry"]["turn"]
            );
            let captured = fixture.loss.lock().unwrap().clone().unwrap();
            assert_eq!(captured.id, decision);
            assert!(
                matches!(captured.command, Command::ResolveApproval { session, approval, decision: sailry_protocol::conversation::Decision::Approve }
                if session == source && json!(approval) == permission["params"]["_meta"]["sailry"]["approval"]["id"])
            );
            let model = child.as_ref().unwrap_or(&fixture.model);
            model.wait_count(2).await;
            let pending = tokio::time::timeout(std::time::Duration::from_secs(10), async {
                loop {
                    let history = fixture
                        .client
                        .read_conversation(source, None, 100)
                        .await
                        .unwrap();
                    if let Some(pending) = history
                        .page
                        .approvals
                        .iter()
                        .find(|approval| approval.state == ApprovalState::Pending)
                    {
                        assert_eq!(
                            history
                                .page
                                .approvals
                                .iter()
                                .filter(|approval| approval.state == ApprovalState::Approved)
                                .count(),
                            1
                        );
                        break pending.clone();
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            assert_eq!(
                std::fs::read_to_string(fixture.root().join("approved.txt")).unwrap(),
                "Admitted decision"
            );
            assert!(!fixture.root().join("pending.txt").exists());
            fixture
                .execute(Command::ResolveApproval {
                    session: source,
                    approval: pending.id,
                    decision: sailry_protocol::conversation::Decision::Approve,
                })
                .await;
            assert_eq!(fixture.settled().await.runs[0].status, Status::Completed);
            model.replace_tools(Vec::new());
            let next = RequestId::new();
            let completed = editor
                .request(
                    4,
                    "session/prompt",
                    prompt(
                        fixture.session.id,
                        next,
                        fixture.session.revision,
                        "Continue after the resolved decision",
                    ),
                    None,
                )
                .await;
            assert_eq!(completed["result"]["stopReason"], "end_turn", "{completed}");
            assert_eq!(
                completed["result"]["_meta"]["sailry"]["request_id"],
                json!(next)
            );
            let page = fixture.page().await;
            assert_eq!(page.runs.len(), 2);
            assert!(page.runs.iter().all(|run| run.status == Status::Completed));
            let history = fixture
                .client
                .read_conversation(source, None, 100)
                .await
                .unwrap();
            assert_eq!(history.page.approvals.len(), 2);
            assert!(
                history
                    .page
                    .approvals
                    .iter()
                    .all(|approval| approval.state == ApprovalState::Approved)
            );
            assert_eq!(fixture.loss.lock().unwrap().as_ref().unwrap().id, decision);
            editor.close().await;
            fixture.close().await;
        }
    }
}

#[tokio::test]
async fn permission_rpc_error_keeps_connection_usable() {
    for remote in [false, true] {
        for delegated in [false, true] {
            let model = models::Server::scripted_tools(vec![(models::plugin_tool("files", "write_file"),
                json!({"path":"pending.txt","text":"Explicit later decision","expected_revision":null}))]).await;
            let (parent, child) = if delegated {
                (
                    models::Server::scripted_tools(vec![(
                        models::plugin_tool("delegation", "spawn_agent"),
                        json!({"role":"writer","task":"Write pending.txt"}),
                    )])
                    .await,
                    Some(model),
                )
            } else {
                (model, None)
            };
            let mut fixture = Fixture::new(remote, parent, false).await;
            if let Some(child) = &child {
                writer(&mut fixture, child).await;
            }
            let mut editor = fixture.editor().await;
            editor.initialize().await;
            editor.load(&fixture).await;
            editor.send(json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":prompt(
                fixture.session.id, RequestId::new(), fixture.session.revision, "Ask before writing pending.txt")}));
            let permission = loop {
                let frame = editor.next().await;
                if frame["method"] == "session/request_permission" {
                    editor.permissions.push(frame["params"].clone());
                    break frame;
                }
                assert_eq!(frame["method"], "session/update", "{frame}");
                editor.updates.push(frame["params"].clone());
            };
            editor.send(json!({"jsonrpc":"2.0","id":permission["id"],
                "error":{"code":-32097,"message":"Fixture editor permission unavailable","data":{"fixture":true}}}));
            let failed = editor.reply(3, None).await;
            let operation = &failed["error"]["data"]["sailry"]["operation"];
            assert_eq!(
                failed["error"]["data"]["sailry"]["state"], "observation_failed",
                "{failed}"
            );
            assert_eq!(operation["rpc_request_id"], permission["id"]);
            assert_eq!(
                operation["decision_request_id"],
                permission["params"]["_meta"]["sailry"]["decision_request_id"]
            );
            assert_eq!(operation["error"]["code"], -32097);
            assert_eq!(operation["error"]["data"]["fixture"], true);
            assert!(operation.get("request_id").is_none());
            let source: SessionId =
                serde_json::from_value(permission["params"]["_meta"]["sailry"]["session"].clone())
                    .unwrap();
            let history = fixture
                .client
                .read_conversation(source, None, 100)
                .await
                .unwrap();
            assert_eq!(history.page.approvals.len(), 1);
            let approval = history.page.approvals[0].clone();
            assert_eq!(
                json!(approval.id),
                permission["params"]["_meta"]["sailry"]["approval"]["id"]
            );
            assert_eq!(approval.session, source);
            assert_eq!(approval.state, ApprovalState::Pending);
            assert_eq!(history.page.runs[0].status, Status::Running);
            assert!(!fixture.root().join("pending.txt").exists());
            fixture.model.replace_tools(Vec::new());
            if let Some(child) = &child {
                child.replace_tools(Vec::new());
            }
            let created = editor
                .request(
                    4,
                    "session/new",
                    json!({"cwd":fixture.root(),"mcpServers":[]}),
                    None,
                )
                .await;
            assert!(created.get("result").is_some(), "{created}");
            let session: SessionId =
                serde_json::from_value(created["result"]["sessionId"].clone()).unwrap();
            let revision = created["result"]["_meta"]["sailry"]["revision"]
                .as_u64()
                .unwrap();
            let completed = editor
                .request(
                    5,
                    "session/prompt",
                    prompt(
                        session,
                        RequestId::new(),
                        revision,
                        "Use the surviving connection",
                    ),
                    None,
                )
                .await;
            assert_eq!(completed["result"]["stopReason"], "end_turn", "{completed}");
            let independent = fixture
                .client
                .read_conversation(session, None, 100)
                .await
                .unwrap();
            assert_eq!(independent.page.runs[0].status, Status::Completed);
            let history = fixture
                .client
                .read_conversation(source, None, 100)
                .await
                .unwrap();
            assert_eq!(history.page.approvals[0].state, ApprovalState::Pending);
            assert_eq!(history.page.runs[0].status, Status::Running);
            fixture
                .execute(Command::ResolveApproval {
                    session: source,
                    approval: approval.id,
                    decision: sailry_protocol::conversation::Decision::Approve,
                })
                .await;
            assert_eq!(fixture.settled().await.runs[0].status, Status::Completed);
            assert_eq!(
                std::fs::read_to_string(fixture.root().join("pending.txt")).unwrap(),
                "Explicit later decision"
            );
            let completed = editor
                .request(
                    6,
                    "session/prompt",
                    prompt(
                        fixture.session.id,
                        RequestId::new(),
                        fixture.session.revision,
                        "Continue the original session",
                    ),
                    None,
                )
                .await;
            assert_eq!(completed["result"]["stopReason"], "end_turn", "{completed}");
            assert_eq!(fixture.page().await.runs.len(), 2);
            let history = fixture
                .client
                .read_conversation(source, None, 100)
                .await
                .unwrap();
            assert_eq!(history.page.approvals.len(), 1);
            assert_eq!(history.page.approvals[0].state, ApprovalState::Approved);
            assert!(fixture.loss.lock().unwrap().is_none());
            editor.close().await;
            fixture.close().await;
        }
    }
}

#[tokio::test]
async fn permission_disconnect_keeps_node_request_pending() {
    for remote in [false, true] {
        let model = models::Server::turn_tools(vec![(
            models::plugin_tool("files", "write_file"),
            json!({"path":"pending.txt","text":"Await the user","expected_revision":null}),
        )])
        .await;
        let fixture = Fixture::new(remote, model, false).await;
        let mut editor = fixture.editor().await;
        editor.initialize().await;
        editor.load(&fixture).await;
        editor.send(json!({"jsonrpc":"2.0","id":3,"method":"session/prompt", "params":prompt(fixture.session.id,
            RequestId::new(), fixture.session.revision, "Ask before writing pending.txt")}));
        let permission = loop {
            let frame = editor.next().await;
            if frame["method"] == "session/request_permission" {
                break frame;
            }
            assert_eq!(frame["method"], "session/update", "{frame}");
        };
        editor.close().await;
        let page = fixture.page().await;
        assert_eq!(page.approvals.len(), 1);
        assert_eq!(
            json!(page.approvals[0].id),
            permission["params"]["_meta"]["sailry"]["approval"]["id"]
        );
        assert_eq!(page.approvals[0].session, fixture.session.id);
        assert_eq!(page.approvals[0].state, ApprovalState::Pending);
        assert_eq!(page.runs[0].status, Status::Running);
        assert!(!fixture.root().join("pending.txt").exists());
        fixture
            .execute(Command::ResolveApproval {
                session: page.approvals[0].session,
                approval: page.approvals[0].id,
                decision: sailry_protocol::conversation::Decision::Deny,
            })
            .await;
        assert_eq!(fixture.settled().await.runs[0].status, Status::Completed);
        assert!(!fixture.root().join("pending.txt").exists());
        fixture.close().await;
    }
}

#[tokio::test]
async fn requires_each_unrestricted_call_in_full_mode() {
    for remote in [false, true] {
        let arguments = json!({"command":"printf '%s\\n' approved >> ../outside.txt","description":"Append the fixture marker",
            "cwd":".","access":"unrestricted","background":false,"timeout_ms":1000});
        let model = models::Server::turn_tools(vec![
            (
                models::plugin_tool("commands", "run_command"),
                arguments.clone(),
            ),
            (models::plugin_tool("commands", "run_command"), arguments),
        ])
        .await;
        let mut fixture = Fixture::new(remote, model, false).await;
        let mut config = fixture.session.config.clone();
        config.permission = sailry_protocol::Permission::Full;
        let Output::Session(session) = fixture
            .execute(Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config,
            })
            .await
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let mut editor = fixture.editor().await;
        editor.initialize().await;
        editor.load(&fixture).await;
        let response = editor
            .request(
                3,
                "session/prompt",
                prompt(
                    fixture.session.id,
                    RequestId::new(),
                    fixture.session.revision,
                    "Run both explicitly approved fixture commands",
                ),
                Some("allow"),
            )
            .await;
        assert!(response.get("result").is_some(), "{response}");
        assert_eq!(editor.permissions.len(), 2);
        assert_ne!(
            editor.permissions[0]["_meta"]["sailry"]["approval"]["id"],
            editor.permissions[1]["_meta"]["sailry"]["approval"]["id"]
        );
        for permission in &editor.permissions {
            assert_eq!(
                permission["_meta"]["sailry"]["approval"]["access"],
                "unrestricted"
            );
            assert_eq!(permission["_meta"]["sailry"]["approval"]["source"], "user");
            assert!(
                permission["toolCall"]["title"]
                    .as_str()
                    .unwrap()
                    .contains("outside the workspace and network access")
            );
            assert!(
                permission["options"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|option| matches!(
                        option["kind"].as_str(),
                        Some("allow_once" | "reject_once")
                    ))
            );
        }
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("outside.txt")).unwrap(),
            "approved\napproved\n"
        );
        let page = fixture.page().await;
        assert_eq!(page.approvals.len(), 2);
        assert!(
            page.approvals
                .iter()
                .all(|approval| approval.state == ApprovalState::Approved)
        );
        editor.close().await;
        fixture.close().await;
    }
}

async fn writer(fixture: &mut Fixture, child: &models::Server) {
    let provider = fixture::provider(&fixture.client, &child.endpoint).await;
    let role = role::Profile {
        id: RoleId::new(),
        revision: 0,
        key: "writer".into(),
        name: "Writer".into(),
        appearance: None,
        description: "Write the requested fixture file".into(),
        read_only: false,
        model: Some(role::Model {
            provider: provider.id,
            model: provider.default_model.clone(),
            effort: None,
        }),
        max_turns: Some(5),
        skills: Vec::new(),
        instructions: "Use the files tool to complete the delegated task".into(),
    };
    let Output::Role(role) = fixture
        .execute(Command::PutRole {
            role,
            expected_revision: 0,
        })
        .await
    else {
        panic!("role expected")
    };
    let Output::Session(session) = fixture
        .execute(Command::SetSessionRoles {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            roles: vec![role.reference()],
        })
        .await
    else {
        panic!("session expected")
    };
    fixture.session = session;
}
