use super::*;
use crate::{Command, NodeId, Request, WorktreeId};

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "preserves_enable_choice_when_unavailable",
            (|scenario: &'static str| {
                let mut summary: Summary = serde_json::from_value(serde_json::json!({
                    "name":"example", "revision":1, "digest":"a", "settings_revision":0,
                    "enabled":true, "version":"0.1.0", "description":null
                }))
                .unwrap();
                assert!(summary.available(), "{scenario}");
                summary.compatibility = Compatibility::RequiresNewer {
                    required: "0.2.0".into(),
                };
                assert!(summary.enabled, "{scenario}");
                assert!(!summary.available(), "{scenario}");
                assert_eq!(
                    summary.compatibility.required(),
                    Some("0.2.0"),
                    "{scenario}"
                );
                let encoded = serde_json::to_vec(&summary).unwrap();
                assert_eq!(
                    serde_json::from_slice::<Summary>(&encoded).unwrap(),
                    summary,
                    "{scenario}"
                );
            }) as fn(&'static str),
        ),
        (
            "distinguishes_preview_and_installation",
            (|scenario: &'static str| {
                let stream = crate::StreamId::new();
                let commands = [
                    (
                        Command::UploadPlugin(UploadSpec {
                            size: 123,
                            revision: "a".repeat(64),
                        }),
                        false,
                    ),
                    (Command::InspectPluginUpload { stream }, false),
                    (
                        Command::InstallPluginUpload {
                            stream,
                            source: UploadSource::Archive,
                            name: "example".into(),
                            expected_revision: 0,
                        },
                        true,
                    ),
                ];
                for (command, durable) in commands {
                    assert_eq!(command.durable(), durable, "{scenario}");
                    let request = Request::new(NodeId([1; 32]), command);
                    assert_eq!(request.version, 1, "{scenario}");
                    assert_eq!(
                        serde_json::from_slice::<Request>(&serde_json::to_vec(&request).unwrap())
                            .unwrap(),
                        request,
                        "{scenario}"
                    );
                }
            }),
        ),
        (
            "request_preserves_scope",
            (|scenario: &'static str| {
                let worktree = WorktreeId::new();
                let original = Request::new(
                    NodeId([1; 32]),
                    Command::WriteFile {
                        worktree,
                        path: "notes.txt".into(),
                        text: "Content".into(),
                        expected_revision: None,
                    },
                );
                let request = original.clone().with_plugin(Context {
                    invocation: None,
                    turn: None,
                    surface: Default::default(),
                    package: Reference {
                        name: "example".into(),
                        digest: "a".repeat(64),
                        settings_revision: 0,
                    },
                    worktree: Some(worktree),
                    session: None,
                });
                assert_eq!(request.id, original.id, "{scenario}");
                assert_eq!(request.command, original.command, "{scenario}");
                assert_eq!(request.version, 1, "{scenario}");
                assert!(request.command.durable(), "{scenario}");
                let encoded = serde_json::to_value(&request).unwrap();
                assert_eq!(
                    encoded["plugin"]["package"]["name"], "example",
                    "{scenario}"
                );
                assert_eq!(
                    serde_json::from_value::<Request>(encoded).unwrap(),
                    request,
                    "{scenario}"
                );
                assert!(
                    serde_json::to_value(original)
                        .unwrap()
                        .get("plugin")
                        .is_none(),
                    "{scenario}"
                );
            }),
        ),
        (
            "redacts_settings_secrets",
            (|scenario: &'static str| {
                use crate::Secret;
                use settings::SecretUpdate;
                use std::collections::BTreeMap;
                let package = Reference {
                    name: "example".into(),
                    digest: "a".repeat(64),
                    settings_revision: 7,
                };
                let request = Request::new(
                    NodeId([1; 32]),
                    Command::SavePluginSettings {
                        package: package.clone(),
                        values: BTreeMap::from([("label".into(), serde_json::json!("中文🙂"))]),
                        secrets: BTreeMap::from([(
                            "token".into(),
                            SecretUpdate::Replace(Secret::new("private-fixture-token".into())),
                        )]),
                    },
                );
                assert_eq!(request.version, 1, "{scenario}");
                assert!(request.command.durable(), "{scenario}");
                assert!(
                    !format!("{request:?}").contains("private-fixture-token"),
                    "{scenario}"
                );
                assert_eq!(
                    serde_json::from_slice::<Request>(&serde_json::to_vec(&request).unwrap())
                        .unwrap(),
                    request,
                    "{scenario}"
                );
                assert!(
                    !Command::ReadPluginSettings { package }.durable(),
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

mod extension {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "validates_optional_market_metadata",
                (|scenario: &'static str| {
                    let empty: Extension =
                        serde_json::from_value(json!({"api_version":"v1"})).unwrap();
                    assert_eq!(empty.category, None, "{scenario}");
                    assert_eq!(empty.tint, None, "{scenario}");
                    for (category, tint) in [
                        ("development", "blue"),
                        ("documents", "violet"),
                        ("data", "teal"),
                        ("automation", "amber"),
                        ("entertainment", "gray"),
                    ] {
                        let extension: Extension = serde_json::from_value(
                            json!({"api_version":"v1", "category":category, "tint":tint}),
                        )
                        .unwrap();
                        let value = serde_json::to_value(extension).unwrap();
                        assert_eq!(value["category"], category, "{scenario}");
                        assert_eq!(value["tint"], tint, "{scenario}");
                    }
                    for value in [
                        json!({"api_version":"v1", "category":"unknown"}),
                        json!({"api_version":"v1", "tint":"red"}),
                    ] {
                        assert!(
                            serde_json::from_value::<Extension>(value).is_err(),
                            "{scenario}"
                        );
                    }
                    let mut notes = catalog::ReleaseNotes {
                        text: "Long notes\n".repeat(1000),
                        locales: Default::default(),
                    };
                    assert!(notes.valid(), "{scenario}");
                    assert_eq!(notes.text("fr"), notes.text, "{scenario}");
                    notes.locales.insert("zh-CN".into(), "变更说明".into());
                    assert_eq!(notes.text("zh-CN"), "变更说明", "{scenario}");
                    notes.locales.insert("fr".into(), "bad\0notes".into());
                    assert!(!notes.valid(), "{scenario}");
                }) as fn(&'static str),
            ),
            (
                "ignores_extra_fields",
                (|scenario: &'static str| {
                    let extension: Extension = serde_json::from_value(json!({
                        "api_version": API_VERSION,
                        "actions": ["files.read"],
                        "unused": {"nested": [1, 2, 3]}
                    }))
                    .unwrap();
                    assert_eq!(extension.actions, vec![Action::ReadFiles], "{scenario}");
                }),
            ),
            (
                "defaults_to_no_grants",
                (|scenario: &'static str| {
                    let extension: Extension =
                        serde_json::from_value(json!({"api_version": API_VERSION})).unwrap();
                    assert!(extension.actions.is_empty(), "{scenario}");
                    assert!(extension.tools.is_empty(), "{scenario}");
                    assert!(extension.ui.is_empty(), "{scenario}");
                    assert!(extension.host.is_none(), "{scenario}");
                    assert!(extension.desktop.is_none(), "{scenario}");
                    assert!(extension.storage.is_none(), "{scenario}");
                }),
            ),
            (
                "requires_explicit_version",
                (|scenario: &'static str| {
                    assert!(
                        serde_json::from_value::<Extension>(json!({"actions": []})).is_err(),
                        "{scenario}"
                    );
                }),
            ),
            (
                "rejects_malformed_grants",
                (|scenario: &'static str| {
                    for actions in [
                        json!(null),
                        json!("files.read"),
                        json!([true]),
                        json!([{"files.read": true}]),
                    ] {
                        assert!(
                            serde_json::from_value::<Extension>(
                                json!({"api_version": API_VERSION, "actions": actions})
                            )
                            .is_err(),
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
}

mod inventory {
    use super::*;
    use serde_json::{Value, json};

    fn value() -> Value {
        json!({
            "summary": {
                "name": "fixture",
                "revision": 1,
                "digest": "a".repeat(64),
                "settings_revision": 0
            }
        })
    }

    #[test]
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "defaults_to_disabled_empty_capabilities",
                (|scenario: &'static str| {
                    let info: Info = serde_json::from_value(value()).unwrap();
                    assert!(!info.summary.enabled, "{scenario}");
                    assert!(info.skills.is_empty(), "{scenario}");
                    assert!(info.mcp.is_empty(), "{scenario}");
                    assert!(info.issues.is_empty(), "{scenario}");
                    assert!(info.extension.is_none(), "{scenario}");
                }) as fn(&'static str),
            ),
            (
                "requires_package_identity_and_revisions",
                (|scenario: &'static str| {
                    for field in ["name", "revision", "digest", "settings_revision"] {
                        let mut body = value();
                        body["summary"].as_object_mut().unwrap().remove(field);
                        assert!(
                            serde_json::from_value::<Info>(body).is_err(),
                            "{scenario}: accepted missing {field}"
                        );
                    }
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}

mod origin {
    use super::*;

    #[test]
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "preserves_source_kind",
                (|scenario: &'static str| {
                    for (source, kind) in [
                        (UploadSource::Directory, "directory"),
                        (UploadSource::Archive, "archive"),
                    ] {
                        let request = Request::new(
                            NodeId([1; 32]),
                            Command::InstallPluginUpload {
                                stream: crate::StreamId::new(),
                                source,
                                name: "example".into(),
                                expected_revision: 3,
                            },
                        );
                        let encoded = serde_json::to_value(&request).unwrap();
                        assert_eq!(encoded["command"]["data"]["source"], kind, "{scenario}");
                        assert_eq!(
                            serde_json::from_value::<Request>(encoded).unwrap(),
                            request,
                            "{scenario}"
                        );
                    }
                }) as fn(&'static str),
            ),
            (
                "requires_upload_source",
                (|scenario: &'static str| {
                    let mut encoded = serde_json::to_value(Command::InstallPluginUpload {
                        stream: crate::StreamId::new(),
                        source: UploadSource::Directory,
                        name: "example".into(),
                        expected_revision: 0,
                    })
                    .unwrap();
                    encoded["data"].as_object_mut().unwrap().remove("source");
                    assert!(
                        serde_json::from_value::<Command>(encoded).is_err(),
                        "{scenario}"
                    );
                }),
            ),
            (
                "encodes_installation_metadata",
                (|scenario: &'static str| {
                    for (origin, kind) in [
                        (Origin::Bundled, "bundled"),
                        (Origin::Directory, "directory"),
                        (Origin::Archive, "archive"),
                        (
                            Origin::Online {
                                source: skills::Resolved {
                                    repository: "https://github.com/owner/repository.git".into(),
                                    git_ref: "main".into(),
                                    commit: "a".repeat(40),
                                },
                                path: "package".into(),
                            },
                            "online",
                        ),
                        (
                            Origin::Worktree {
                                worktree: WorktreeId::new(),
                                path: "package".into(),
                            },
                            "worktree",
                        ),
                    ] {
                        let encoded = serde_json::to_value(&origin).unwrap();
                        assert_eq!(encoded["kind"], kind, "{scenario}");
                        assert_eq!(
                            serde_json::from_value::<Origin>(encoded).unwrap(),
                            origin,
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
}
