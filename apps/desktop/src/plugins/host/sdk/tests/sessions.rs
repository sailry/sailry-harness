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
