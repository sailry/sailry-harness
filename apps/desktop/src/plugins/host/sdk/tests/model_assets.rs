use super::*;
use sailry_protocol::{
    installation::{ModelRef, Target},
    models::{Asset, Definition, Format, Hub, Source},
};

fn host(fixture: &Fixture) -> Arc<Host> {
    fixture.package();
    let path = fixture.directory.path().join("project/package/plugin.json");
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest["extensions"]["dev.sailry.platform"]["actions"] =
        json!(["models.read", "models.install"]);
    std::fs::write(path, manifest.to_string()).unwrap();
    let package = fixture.install(0);
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    Host::new(
        fixture.binding.client.clone(),
        Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: package.summary.reference(),
            worktree: Some(fixture.session.worktree),
            session: Some(fixture.session.id),
        },
        fixture.runtime.handle().clone(),
        false,
        None,
    )
}

fn definition() -> Definition {
    let source = Source {
        hub: Hub::HuggingFace,
        repository: "fixture/speech".into(),
        revision: "a".repeat(40),
    };
    Definition {
        source: source.clone(),
        name: "Fixture speech".into(),
        license: "Apache-2.0".into(),
        format: Format::SenseVoice {},
        query_prefix: None,
        document_prefix: None,
        assets: ["model.onnx", "tokens.txt"]
            .into_iter()
            .map(|name| Asset {
                source: source.clone(),
                path: name.into(),
                name: name.into(),
                bytes: 4,
                sha256: "b".repeat(64),
            })
            .collect(),
    }
}

#[test]
fn captures_node_registration_before_installation() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let host = host(&fixture);
        let models = host.models_module();
        let sdk = host.sdk();
        let tasks = host.installations_module();
        models.validate().unwrap();
        tasks.validate().unwrap();
        let Output::KnowledgeModels(current) = fixture
            .runtime
            .block_on(
                fixture
                    .binding
                    .client
                    .execute(fixture.binding.client.prepare(Command::ReadKnowledgeModels)),
            )
            .unwrap()
        else {
            panic!("model catalog expected");
        };
        let id = call(
            &models,
            "register",
            json!([{"target":"node","definition":current[0].definition}]),
        );
        let registered = complete(&fixture, &sdk, "completeRequest", json!([id]));
        assert_eq!(registered["Ok"]["kind"], "knowledge_model");
        assert_eq!(
            complete(&fixture, &sdk, "completeRequest", json!([id])),
            registered
        );
        let model: sailry_protocol::knowledge::Model =
            serde_json::from_value(registered["Ok"]["data"]["model"].clone()).unwrap();
        let prepared = call(
            &models,
            "prepare",
            json!([ModelRef::Embedding {
                target: Target::Node,
                model
            }]),
        );
        let request = host
            .prepared(prepared.as_str().unwrap().parse().unwrap())
            .unwrap();
        assert_eq!(request.target, fixture.binding.client.target());
        assert_eq!(request.plugin, Some(host.context()));
        assert!(
            matches!(request.command, Command::InstallKnowledgeModel { model: captured } if captured == model)
        );
        assert!(
            !fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| matches!(request.command, Command::InstallKnowledgeModel { .. }))
        );
        assert!(
            tasks
                .begin("read", &args(json!([[RequestId::new()]])))
                .is_err()
        );
        assert!(tasks.begin("read", &args(json!([[id]]))).is_err());
        host.close();
        assert!(
            models
                .call(
                    "prepare",
                    &args(json!([ModelRef::Embedding {
                        target: Target::Node,
                        model
                    }]))
                )
                .is_err()
        );
        fixture.close();
    }
}

#[test]
fn controller_registration_stays_on_this_device() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let host = host(&fixture);
        let directory = tempfile::tempdir().unwrap();
        let manager = crate::dictation::models::Manager::open(directory.path().into()).unwrap();
        host.model_services(Some(manager.clone()), fixture.runtime.clone(), true);
        let models = host.models_module();
        let sdk = host.sdk();
        let tasks = host.installations_module();
        let before = fixture.transport.requests.lock().unwrap().len();
        let id = call(
            &models,
            "register",
            json!([{"target":"controller","definition":definition()}]),
        );
        let receipt = complete(&fixture, &sdk, "completeRequest", json!([id]));
        assert_eq!(receipt["Ok"]["kind"], "model_registered");
        assert_eq!(
            complete(&fixture, &sdk, "completeRequest", json!([id])),
            receipt
        );
        let model = receipt["Ok"]["data"]["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(manager.entries()[0].record.id, model);
        assert!(!manager.entries()[0].record.installed);
        let prepared = call(
            &models,
            "prepare",
            json!([ModelRef::Speech {
                target: Target::Controller,
                id: Some(model)
            }]),
        );
        assert!(
            host.prepared(prepared.as_str().unwrap().parse().unwrap())
                .is_err()
        );
        assert_eq!(
            complete(&fixture, &tasks, "read", json!([[prepared]])),
            json!([])
        );
        assert!(
            tasks
                .begin("read", &args(json!([[RequestId::new()]])))
                .is_err()
        );
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), before);
        host.close();
        assert!(tasks.begin("read", &args(json!([[prepared]]))).is_err());
        fixture.close();
    }
}

#[test]
fn missing_controller_and_denied_grant_are_distinct() {
    let fixture = Fixture::new(false);
    let host = host(&fixture);
    let models = host.models_module();
    host.model_services(None, fixture.runtime.clone(), true);
    let input = json!([ModelRef::Speech {
        target: Target::Controller,
        id: None
    }]);
    let error = models.call("prepare", &args(input.clone())).unwrap_err();
    assert_eq!(values::fault_code(&error.to_string()), "unavailable");
    let directory = tempfile::tempdir().unwrap();
    let manager = crate::dictation::models::Manager::open(directory.path().into()).unwrap();
    host.model_services(Some(manager), fixture.runtime.clone(), false);
    let error = models.call("prepare", &args(input)).unwrap_err();
    assert_eq!(values::fault_code(&error.to_string()), "permission_denied");
    fixture.close();
}
