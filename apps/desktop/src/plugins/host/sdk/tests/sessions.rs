use super::*;

#[test]
fn uses_public_receipts() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let path = fixture.directory.path().join("project/package/plugin.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["actions"] =
            json!(["projects.read", "sessions.start"]);
        std::fs::write(path, manifest.to_string()).unwrap();
        let package = fixture.install(0);
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let host = Host::new(
            fixture.binding.client.clone(),
            Context {
                invocation: None,
                turn: None,
                surface: Default::default(),
                package: package.summary.reference(),
                worktree: None,
                session: None,
            },
            fixture.runtime.handle().clone(),
            false,
            None,
        );
        let module = host.sdk();
        module.validate().unwrap();
        let catalog = complete(&fixture, &module, "readProjectCatalog", json!([]));
        assert_eq!(
            catalog["projects"][0]["id"],
            json!(fixture.session.project.unwrap())
        );
        assert!(catalog.get("sessions").is_none());
        let id = call(
            &module,
            "startSession",
            json!([{
                "project":fixture.session.project,"worktree":fixture.session.worktree,"config":null,
                "title":"Scheduled start","message":{"text":"Run the review","attachments":[]}
            }]),
        );
        let result = complete(&fixture, &module, "completeRequest", json!([id]));
        assert_eq!(result["Err"]["code"], "not_configured");
        assert_eq!(
            complete(&fixture, &module, "completeRequest", json!([id])),
            result
        );
        call(&module, "forgetRequest", json!([id]));
        assert!(host.requests.lock().unwrap().is_empty());
        host.close();
        assert!(
            module
                .begin("readProjectCatalog", &args(json!([])))
                .is_err()
        );
        fixture.close();
    }
}

#[test]
fn captures_selected_models() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let path = fixture.directory.path().join("project/package/plugin.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["actions"] = json!(["models.read"]);
        std::fs::write(path, manifest.to_string()).unwrap();
        let package = fixture.install(0);
        let context = Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: package.summary.reference(),
            worktree: None,
            session: None,
        };
        let host = Host::new(
            fixture.binding.client.clone(),
            context.clone(),
            fixture.runtime.handle().clone(),
            false,
            None,
        );
        let module = host.sdk();
        module.validate().unwrap();
        let catalog = complete(&fixture, &module, "listModels", json!([]));
        let selected = catalog["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|model| model["model"] == "fixture")
            .unwrap()["id"]
            .clone();
        let config = complete(&fixture, &module, "resolveSessionModel", json!([selected]));
        assert_eq!(config["provider"], json!(fixture.session.config.provider));
        assert_eq!(config["model"], "fixture");
        assert_eq!(config["permission"], "ask");
        assert_eq!(config["mode"], "code");
        let mut base = fixture.session.config.clone();
        base.permission = sailry_protocol::Permission::Full;
        base.mode = sailry_protocol::WorkMode::Plan;
        let changed = complete(
            &fixture,
            &module,
            "resolveSessionModel",
            json!([selected, "default", base]),
        );
        assert_eq!(changed["permission"], "full");
        assert_eq!(changed["mode"], "plan");
        assert_eq!(changed["effort"], "default");
        assert!(host.requests.lock().unwrap().is_empty());
        let requests = fixture.transport.requests.lock().unwrap();
        let resolved = requests
            .iter()
            .filter(|request| matches!(request.command, Command::ResolvePluginModel { .. }))
            .collect::<Vec<_>>();
        assert_eq!(resolved.len(), 2);
        assert!(
            resolved
                .iter()
                .all(|request| request.target == fixture.node.id()
                    && request.plugin.as_ref() == Some(&context)
                    && !request.command.durable())
        );
        drop(requests);
        host.close();
        assert!(
            module
                .begin("resolveSessionModel", &args(json!([selected])))
                .is_err()
        );
        fixture.close();
    }
}

#[test]
fn restores_owned_sessions_and_controls_execution() {
    use sailry_protocol::{TeamId, team};
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let path = fixture.directory.path().join("project/package/plugin.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["actions"] = json!([
            "sessions.start",
            "teams.read",
            "conversation.read",
            "conversation.control"
        ]);
        std::fs::write(path, manifest.to_string()).unwrap();
        let package = fixture.install(0);
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let Output::Team(team) = fixture.execute(Command::PutTeam {
            team: team::Profile {
                id: TeamId::new(),
                revision: 0,
                key: "sdk".into(),
                name: "SDK".into(),
                appearance: None,
                description: String::new(),
                model: None,
                instructions: "Complete the task".into(),
                members: vec![],
            },
            expected_revision: 0,
        }) else {
            panic!("team expected")
        };
        let host = Host::new(
            fixture.binding.client.clone(),
            Context {
                invocation: None,
                turn: None,
                surface: Default::default(),
                package: package.summary.reference(),
                worktree: None,
                session: None,
            },
            fixture.runtime.handle().clone(),
            false,
            None,
        );
        let module = host.sdk();
        module.validate().unwrap();
        let teams = complete(&fixture, &module, "listTeams", json!([]));
        assert!(
            teams
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value["id"] == json!(team.id)
                    && value["revision"] == team.revision.to_string())
        );
        let association = json!({"kind":"schedule","id":"daily"});
        let id = call(
            &module,
            "startSession",
            json!([{
                "project":fixture.session.project,"worktree":fixture.session.worktree,
                "config":fixture.session.config,"title":"SDK start","message":{"text":"Run","attachments":[]},
                "execution":"team","team":{"id":team.id,"revision":team.revision.to_string()},"association":association
            }]),
        );
        let admitted = complete(&fixture, &module, "completeRequest", json!([id]));
        assert!(admitted.get("Ok").is_some(), "{admitted}");
        assert_eq!(
            admitted["Ok"]["data"]["roles"]["team"]["revision"],
            team.revision.to_string()
        );
        assert_eq!(
            complete(&fixture, &module, "completeRequest", json!([id])),
            admitted
        );
        let session = admitted["Ok"]["data"]["session"].clone();
        let page = complete(
            &fixture,
            &module,
            "listAssociatedSessions",
            json!([association, null, 1]),
        );
        assert_eq!(page["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(page["sessions"][0]["id"], session);
        assert_eq!(
            page["sessions"][0]["association"]["namespace"],
            "project-summary"
        );
        let read = complete(&fixture, &module, "readSession", json!([session]));
        assert_eq!(read["roles"]["execution"], "team");
        let changed = call(
            &module,
            "setSessionExecution",
            json!([{"execution":"model"},read["revision"],session]),
        );
        let changed = complete(&fixture, &module, "completeRequest", json!([changed]));
        assert!(changed.get("Ok").is_some(), "{changed}");
        assert_eq!(changed["Ok"]["data"]["roles"]["execution"], "model");
        assert_eq!(changed["Ok"]["data"]["roles"]["profiles"], json!([]));
        let stale = call(
            &module,
            "setSessionExecution",
            json!([{"execution":"team"},read["revision"],session]),
        );
        assert_eq!(
            complete(&fixture, &module, "completeRequest", json!([stale]))["Err"]["code"],
            "revision_conflict"
        );
        let sent = call(
            &module,
            "sendMessage",
            json!([{"text":"Continue","attachments":[]},changed["Ok"]["data"]["revision"],session]),
        );
        let sent = complete(&fixture, &module, "completeRequest", json!([sent]));
        assert!(sent.get("Ok").is_some(), "{sent}");
        assert_eq!(sent["Ok"]["data"]["roles"]["execution"], "model");
        let history = complete(
            &fixture,
            &module,
            "readConversation",
            json!([null, 20, session]),
        );
        assert_eq!(history["page"]["session"], session);
        let unrelated = module
            .begin("readSession", &args(json!([fixture.session.id])))
            .unwrap();
        let error = fixture.runtime.block_on(unrelated).unwrap_err();
        assert_eq!(values::fault_code(&error.to_string()), "permission_denied");
        let error = module
            .begin(
                "listAssociatedSessions",
                &args(json!([{"namespace":"other","kind":"schedule","id":"daily"}])),
            )
            .err()
            .expect("foreign namespace must be rejected");
        assert_eq!(values::fault_code(&error.to_string()), "permission_denied");
        host.close();
        fixture.close();
    }
}

#[test]
fn preserves_dispatch_revision_shapes() {
    use sailry_protocol::{SessionId, TeamId, conversation, dispatch};
    let handler = dispatch::Handler {
        name: "run".into(),
        revision: 7,
        enabled: true,
        source: dispatch::Source {
            package: "fixture".into(),
            topic: "tick".into(),
        },
        queue: "tasks".into(),
        callback: dispatch::Callback {
            scope: Default::default(),
            completion: dispatch::Completion::Command,
            command: Box::new(Command::SetSessionExecution {
                session: SessionId::new(),
                expected_revision: 1,
                execution: conversation::Execution::Team,
                team: Some(sailry_protocol::team::Selection {
                    id: TeamId::new(),
                    revision: Some(9_007_199_254_740_993),
                }),
                models: Default::default(),
            }),
            bindings: Default::default(),
        },
    };
    for dispatch in [
        dispatch::Output::Handler(handler.clone()),
        dispatch::Output::Handlers(vec![handler]),
    ] {
        let output = public_output(Output::Dispatch(dispatch)).unwrap();
        let record = if output["data"]["kind"] == "handlers" {
            &output["data"]["data"][0]
        } else {
            &output["data"]["data"]
        };
        assert_eq!(record["revision"], 7);
        assert_eq!(
            record["callback"]["command"]["data"]["team"]["revision"],
            "9007199254740993"
        );
    }
    let output = public_output(Output::Dispatch(dispatch::Output::Schedule(
        dispatch::Schedule {
            id: sailry_protocol::ScheduleId::new(),
            revision: 7,
            enabled: false,
            topic: "tick".into(),
            payload: Value::Null,
            timing: dispatch::Timing::Once { at_ms: 1 },
            next_ms: None,
        },
    )))
    .unwrap();
    assert_eq!(output["data"]["data"]["revision"], 7);
}
