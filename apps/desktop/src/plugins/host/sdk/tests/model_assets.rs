use super::*;
use sailry_protocol::{
    installation::ModelRef,
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
fn registered_ownership() {
    for (scenario, check) in [
        (
            "captures_node_registration_before_installation",
            (|_scenario: &str| {
                for remote in [false, true] {
                    let fixture = Fixture::new(remote);
                    let host = host(&fixture);
                    let models = host.models_module();
                    let sdk = host.sdk();
                    let tasks = host.installations_module();
                    models.validate().unwrap();
                    tasks.validate().unwrap();
                    let Output::LocalModels(sailry_protocol::models::Catalog {
                        models: current,
                        ..
                    }) =
                        fixture
                            .runtime
                            .block_on(fixture.binding.client.execute(
                                fixture.binding.client.prepare(Command::ReadLocalModels {
                                    kind: Some(sailry_protocol::models::Kind::Embedding),
                                }),
                            ))
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
                    assert_eq!(
                        registered["Ok"]["kind"], "local_model",
                        "captures node registration before installation"
                    );
                    assert_eq!(
                        complete(&fixture, &sdk, "completeRequest", json!([id])),
                        registered,
                        "captures node registration before installation"
                    );
                    let model: String =
                        serde_json::from_value(registered["Ok"]["data"]["id"].clone()).unwrap();
                    let prepared = call(
                        &models,
                        "prepare",
                        json!([ModelRef::Node {
                            kind: sailry_protocol::models::Kind::Embedding,
                            id: model.clone()
                        }]),
                    );
                    let request = host
                        .prepared(prepared.as_str().unwrap().parse().unwrap())
                        .unwrap();
                    assert_eq!(
                        request.target,
                        fixture.binding.client.target(),
                        "captures node registration before installation"
                    );
                    assert_eq!(
                        request.plugin,
                        Some(host.context()),
                        "captures node registration before installation"
                    );
                    assert!(
                        matches!(request.command, Command::InstallLocalModel { id: captured } if captured == model),
                        "captures node registration before installation"
                    );
                    assert!(
                        !fixture
                            .transport
                            .requests
                            .lock()
                            .unwrap()
                            .iter()
                            .any(|request| matches!(
                                request.command,
                                Command::InstallLocalModel { .. }
                            )),
                        "captures node registration before installation"
                    );
                    assert!(
                        tasks
                            .begin("read", &args(json!([[RequestId::new()]])))
                            .is_err(),
                        "captures node registration before installation"
                    );
                    assert!(
                        tasks.begin("read", &args(json!([[id]]))).is_err(),
                        "captures node registration before installation"
                    );
                    host.close();
                    assert!(
                        models
                            .call(
                                "prepare",
                                &args(json!([ModelRef::Node {
                                    kind: sailry_protocol::models::Kind::Embedding,
                                    id: model.clone()
                                }]))
                            )
                            .is_err(),
                        "captures node registration before installation"
                    );
                    fixture.close();
                }
            }) as fn(&str),
        ),
        (
            "lists_node_kinds_in_the_captured_scope",
            (|_scenario: &str| {
                for remote in [false, true] {
                    let fixture = Fixture::new(remote);
                    let host = host(&fixture);
                    let module = host.models_module();
                    for kind in [
                        None,
                        Some(sailry_protocol::models::Kind::Embedding),
                        Some(sailry_protocol::models::Kind::Ocr),
                        Some(sailry_protocol::models::Kind::Decision),
                    ] {
                        let Output::LocalModels(sailry_protocol::models::Catalog {
                            models: expected,
                            ..
                        }) = fixture
                            .runtime
                            .block_on(
                                fixture.binding.client.execute(
                                    fixture
                                        .binding
                                        .client
                                        .prepare(Command::ReadLocalModels { kind }),
                                ),
                            )
                            .unwrap()
                        else {
                            panic!("local catalog expected")
                        };
                        let listed = complete(
                            &fixture,
                            &module,
                            "list",
                            json!([{"target":"node","kind":kind}]),
                        );
                        assert_eq!(
                            listed,
                            serde_json::to_value(expected).unwrap(),
                            "lists node kinds in the captured scope"
                        );
                    }
                    assert!(
                        module
                            .begin("list", &args(json!([{"target":"node","kind":"speech"}])))
                            .is_err(),
                        "lists node kinds in the captured scope"
                    );
                    host.close();
                    fixture.close();
                }
            }),
        ),
        (
            "controller_registration_stays_on_this_device",
            (|_scenario: &str| {
                for remote in [false, true] {
                    let fixture = Fixture::new(remote);
                    let host = host(&fixture);
                    let directory = tempfile::tempdir().unwrap();
                    let manager =
                        crate::dictation::models::Manager::open(directory.path().into()).unwrap();
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
                    assert_eq!(
                        receipt["Ok"]["kind"], "model_registered",
                        "controller registration stays on this device"
                    );
                    assert_eq!(
                        complete(&fixture, &sdk, "completeRequest", json!([id])),
                        receipt,
                        "controller registration stays on this device"
                    );
                    let model = receipt["Ok"]["data"]["id"]
                        .as_str()
                        .unwrap()
                        .parse()
                        .unwrap();
                    assert_eq!(
                        manager.entries()[0].record.id,
                        model,
                        "controller registration stays on this device"
                    );
                    assert!(
                        !manager.entries()[0].record.installed,
                        "controller registration stays on this device"
                    );
                    let prepared = call(
                        &models,
                        "prepare",
                        json!([ModelRef::Controller {
                            kind: sailry_protocol::models::Kind::Speech,
                            id: Some(model)
                        }]),
                    );
                    assert!(
                        host.prepared(prepared.as_str().unwrap().parse().unwrap())
                            .is_err(),
                        "controller registration stays on this device"
                    );
                    assert_eq!(
                        complete(&fixture, &tasks, "read", json!([[prepared]])),
                        json!([]),
                        "controller registration stays on this device"
                    );
                    assert!(
                        tasks
                            .begin("read", &args(json!([[RequestId::new()]])))
                            .is_err(),
                        "controller registration stays on this device"
                    );
                    assert_eq!(
                        fixture.transport.requests.lock().unwrap().len(),
                        before,
                        "controller registration stays on this device"
                    );
                    host.close();
                    assert!(
                        tasks.begin("read", &args(json!([[prepared]]))).is_err(),
                        "controller registration stays on this device"
                    );
                    fixture.close();
                }
            }),
        ),
        (
            "missing_controller_and_denied_grant_are_distinct",
            (|_scenario: &str| {
                let fixture = Fixture::new(false);
                let host = host(&fixture);
                let models = host.models_module();
                host.model_services(None, fixture.runtime.clone(), true);
                let input = json!([ModelRef::Controller {
                    kind: sailry_protocol::models::Kind::Speech,
                    id: None
                }]);
                let error = models.call("prepare", &args(input.clone())).unwrap_err();
                assert_eq!(
                    values::fault_code(&error.to_string()),
                    "unavailable",
                    "missing controller and denied grant are distinct"
                );
                let directory = tempfile::tempdir().unwrap();
                let manager =
                    crate::dictation::models::Manager::open(directory.path().into()).unwrap();
                host.model_services(Some(manager), fixture.runtime.clone(), false);
                let error = models.call("prepare", &args(input)).unwrap_err();
                assert_eq!(
                    values::fault_code(&error.to_string()),
                    "permission_denied",
                    "missing controller and denied grant are distinct"
                );
                fixture.close();
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
