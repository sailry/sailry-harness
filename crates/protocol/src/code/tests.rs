use super::*;

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "defaults_missing_editor_switch",
            (|scenario: &'static str| {
                let mut value = serde_json::to_value(Settings::default()).unwrap();
                value.as_object_mut().unwrap().remove("editor_enabled");
                value["after_edit"] = false.into();
                let settings: Settings = serde_json::from_value(value).unwrap();
                assert!(!settings.editor_enabled, "{scenario}");
                assert!(settings.enabled, "{scenario}");
                assert!(!settings.after_edit, "{scenario}");
                let mut value = serde_json::to_value(&settings).unwrap();
                value["editor_enabled"] = true.into();
                assert!(
                    serde_json::from_value::<Settings>(value.clone())
                        .unwrap()
                        .editor_enabled,
                    "{scenario}"
                );
                value["editor_enabled"] = "true".into();
                assert!(
                    serde_json::from_value::<Settings>(value).is_err(),
                    "{scenario}"
                );
            }) as fn(&'static str),
        ),
        (
            "defaults_missing_master_switch",
            (|scenario: &'static str| {
                let mut value = serde_json::to_value(Settings::default()).unwrap();
                value.as_object_mut().unwrap().remove("enabled");
                value["after_edit"] = false.into();
                let settings: Settings = serde_json::from_value(value).unwrap();
                assert!(settings.enabled, "{scenario}");
                assert!(!settings.after_edit, "{scenario}");
                let mut value = serde_json::to_value(&settings).unwrap();
                value["enabled"] = false.into();
                assert!(
                    !serde_json::from_value::<Settings>(value).unwrap().enabled,
                    "{scenario}"
                );
            }),
        ),
        (
            "preserves_retired_declarations_as_data",
            (|scenario: &'static str| {
                use crate::{plugin::Service, tool::Operation};
                use serde_json::json;
                let service: Service = serde_json::from_value(json!("language")).unwrap();
                assert!(!service.current(), "{scenario}");
                assert_eq!(
                    serde_json::to_value(service).unwrap(),
                    json!("language"),
                    "{scenario}"
                );
                let schema = serde_json::to_value(schemars::schema_for!(Operation)).unwrap();
                let schema = serde_json::to_string(&schema).unwrap();
                for name in [
                    "code.packages",
                    "code.catalog",
                    "code.servers",
                    "code.start",
                    "code.stop",
                    "code.install",
                    "code.uninstall",
                    "code.diagnostics",
                    "code.definition",
                    "code.references",
                    "code.hover",
                    "code.symbols",
                ] {
                    let operation: Operation = serde_json::from_value(json!(name)).unwrap();
                    assert!(!operation.current(), "{scenario}");
                    assert_eq!(
                        serde_json::to_value(operation).unwrap(),
                        json!(name),
                        "{scenario}"
                    );
                    assert!(!schema.contains(name), "{scenario}");
                }
                let schema = serde_json::to_value(schemars::schema_for!(Service)).unwrap();
                assert!(
                    !serde_json::to_string(&schema).unwrap().contains("language"),
                    "{scenario}"
                );
            }),
        ),
        (
            "rejects_retired_executable_endpoints",
            (|scenario: &'static str| {
                use serde_json::json;
                for name in [
                    "read_code",
                    "list_code_servers",
                    "list_code_packages",
                    "install_code_server",
                    "cancel_code_install",
                    "cancel_code_query",
                    "read_code_catalog",
                    "list_code_installations",
                    "uninstall_code_server",
                    "read_code_rules",
                    "update_code_rules",
                    "read_code_settings",
                    "save_code_settings",
                    "start_code_server",
                    "stop_code_server",
                ] {
                    assert!(
                        serde_json::from_value::<crate::Command>(json!({"kind":name,"data":{}}))
                            .is_err(),
                        "{scenario}"
                    );
                }
                for name in ["code_servers", "code_installations"] {
                    assert!(
                        serde_json::from_value::<crate::Topic>(json!({"kind":name,"data":{}}))
                            .is_err(),
                        "{scenario}"
                    );
                    assert!(
                        serde_json::from_value::<crate::Update>(json!({"kind":name,"data":{}}))
                            .is_err(),
                        "{scenario}"
                    );
                }
            }),
        ),
        (
            "reads_persisted_completion_outputs",
            (|scenario: &'static str| {
                use crate::{Output, RequestId, RequestOutcome};
                let startup = Startup {
                    worktree: crate::WorktreeId::new(),
                    root: String::new(),
                    server: "retired".into(),
                    executable: "retired".into(),
                    revision: "a".repeat(64),
                };
                for result in [
                    Output::CodeSettings(Settings::default()),
                    Output::CodeServer(Server {
                        startup,
                        status: Status::Ready,
                        progress: None,
                        diagnostics_revision: 1,
                    }),
                    Output::CodeInstallation(Installation {
                        package: "retired".into(),
                        status: InstallStatus::Ready,
                        progress: None,
                        receipt: None,
                        error: None,
                    }),
                    Output::CodeUninstalled {
                        package: "retired".into(),
                        version: "1.0.0".into(),
                    },
                    Output::CodeRules(Rules {
                        helix_commit: "a".repeat(40),
                        mason_tag: "v1".into(),
                        retrieved: "2026-01-01".into(),
                        cached: true,
                    }),
                ] {
                    let output = Output::RequestOutcome {
                        id: RequestId::new(),
                        outcome: RequestOutcome::Completed(Box::new(Ok(result))),
                    };
                    assert_eq!(
                        serde_json::from_value::<Output>(serde_json::to_value(&output).unwrap())
                            .unwrap(),
                        output,
                        "{scenario}"
                    );
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
