use super::*;

#[test]
fn preserves_captured_sources_and_cas_receipts() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let path = fixture.directory.path().join("project/package/plugin.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["actions"] =
            json!(["knowledge.read", "knowledge.write", "files.read"]);
        std::fs::write(path, manifest.to_string()).unwrap();
        let package = fixture.install(0);
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let client = &fixture.binding.client;
        let Output::LocalModels(models) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::ReadLocalModels {
                kind: Some(sailry_protocol::models::Kind::Embedding),
            })))
            .unwrap()
        else {
            panic!("knowledge models expected")
        };
        let Output::LocalModel { id: model } = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::RegisterLocalModel {
                definition: models[0].definition.clone(),
            })))
            .unwrap()
        else {
            panic!("registered model expected")
        };
        assert!(sailry_protocol::models::valid_id(&model));
        let context = Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: package.summary.reference(),
            worktree: Some(fixture.session.worktree),
            session: Some(fixture.session.id),
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
        let draft =
            json!({"name":"Notes","source":{"kind":"worktree","path":"notes.txt"},"model":model});
        let id = call(&module, "prepareKnowledgeCollection", json!([draft]));
        let created = complete(&fixture, &module, "completeRequest", json!([id]));
        assert_eq!(created["Ok"]["kind"], "knowledge_collection");
        assert_eq!(created["Ok"]["data"]["revision"], "1");
        assert_eq!(
            complete(&fixture, &module, "completeRequest", json!([id])),
            created
        );
        let collection = &created["Ok"]["data"];
        assert_eq!(collection["definition"]["model"], json!(model));
        assert_eq!(
            collection["owner"],
            json!({"kind":"plugin","name":package.summary.name})
        );
        assert_eq!(
            collection["definition"]["source"],
            json!({"kind":"worktree","worktree":fixture.session.worktree,"path":"notes.txt"})
        );
        let listed = complete(&fixture, &module, "listKnowledgeCollections", json!([]));
        assert!(
            listed
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["id"] == collection["id"]
                    && entry["revision"] == "1"
                    && entry["definition"]["model"] == json!(model))
        );
        for injected in [
            json!({"name":"Notes","source":{"kind":"directory","path":"/tmp"}}),
            json!({"name":"Notes","source":{"kind":"worktree","path":"notes.txt","worktree":sailry_protocol::WorktreeId::new()}}),
            json!({"name":"Notes","source":{"kind":"worktree","path":"notes.txt"},"shared_with":["other"]}),
        ] {
            assert!(
                module
                    .call("prepareKnowledgeCollection", &args(json!([injected])))
                    .is_err()
            );
        }
        let rebuild = call(
            &module,
            "prepareKnowledgeRebuild",
            json!([collection["id"], collection["revision"]]),
        );
        let rebuilt = complete(&fixture, &module, "completeRequest", json!([rebuild]));
        assert_eq!(rebuilt["Ok"]["data"]["revision"], "2");
        let stale = call(
            &module,
            "prepareKnowledgeRemoval",
            json!([collection["id"], collection["revision"]]),
        );
        assert_eq!(
            complete(&fixture, &module, "completeRequest", json!([stale]))["Err"]["code"],
            "revision_conflict"
        );
        let remove = call(
            &module,
            "prepareKnowledgeRemoval",
            json!([collection["id"], rebuilt["Ok"]["data"]["revision"]]),
        );
        assert_eq!(
            complete(&fixture, &module, "completeRequest", json!([remove]))["Ok"]["kind"],
            "knowledge_collection_deleted"
        );
        assert!(fixture.directory.path().join("project/notes.txt").is_file());
        let raw = "  Original query 東京  ";
        let page = complete(
            &fixture,
            &module,
            "queryKnowledge",
            json!([{"query":raw,"mode":"keyword"}]),
        );
        assert_eq!(page["mode"], "keyword");
        assert_eq!(page["untrusted"], true);
        assert!(page["hits"].as_array().unwrap().is_empty());
        {
            let requests = fixture.transport.requests.lock().unwrap();
            assert!(
                requests
                    .iter()
                    .all(|request| request.plugin.as_ref() == Some(&context))
            );
            assert!(requests.iter().any(|request| matches!(&request.command,Command::QueryKnowledge(query) if query.query == raw)));
        }
        host.close();
        assert!(
            module
                .call("prepareKnowledgeCollection", &args(json!([draft])))
                .is_err()
        );
        assert!(
            module
                .begin("listKnowledgeCollections", &args(json!([])))
                .is_err()
        );
        fixture.close();
    }
}
