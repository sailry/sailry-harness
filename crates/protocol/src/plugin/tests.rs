use super::*;
use crate::{Command, NodeId, Request, WorktreeId};

mod extension {
    use super::*;
    use serde_json::json;

    #[test]
    fn ignores_extra_fields() {
        let extension: Extension = serde_json::from_value(json!({
            "api_version": API_VERSION,
            "actions": ["files.read"],
            "unused": {"nested": [1, 2, 3]}
        }))
        .unwrap();
        assert_eq!(extension.actions, vec![Action::ReadFiles]);
    }

    #[test]
    fn defaults_to_no_grants() {
        let extension: Extension =
            serde_json::from_value(json!({"api_version": API_VERSION})).unwrap();
        assert!(extension.actions.is_empty());
        assert!(extension.tools.is_empty());
        assert!(extension.model_tools.is_empty());
        assert!(extension.ui.is_empty());
        assert!(extension.host.is_none());
        assert!(extension.desktop.is_none());
        assert!(extension.storage.is_none());
    }

    #[test]
    fn requires_explicit_version() {
        assert!(serde_json::from_value::<Extension>(json!({"actions": []})).is_err());
    }

    #[test]
    fn rejects_malformed_grants() {
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
                .is_err()
            );
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
    fn defaults_to_disabled_empty_capabilities() {
        let info: Info = serde_json::from_value(value()).unwrap();
        assert!(!info.summary.enabled);
        assert!(info.skills.is_empty());
        assert!(info.mcp.is_empty());
        assert!(info.issues.is_empty());
        assert!(info.extension.is_none());
    }

    #[test]
    fn requires_package_identity_and_revisions() {
        for field in ["name", "revision", "digest", "settings_revision"] {
            let mut body = value();
            body["summary"].as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<Info>(body).is_err(),
                "accepted missing {field}"
            );
        }
    }
}

#[test]
fn distinguishes_preview_and_installation() {
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
        assert_eq!(command.durable(), durable);
        let request = Request::new(NodeId([1; 32]), command);
        assert_eq!(request.version, 1);
        assert_eq!(
            serde_json::from_slice::<Request>(&serde_json::to_vec(&request).unwrap()).unwrap(),
            request
        );
    }
}

mod origin {
    use super::*;

    #[test]
    fn preserves_source_kind() {
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
            assert_eq!(encoded["command"]["data"]["source"], kind);
            assert_eq!(serde_json::from_value::<Request>(encoded).unwrap(), request);
        }
    }

    #[test]
    fn requires_upload_source() {
        let mut encoded = serde_json::to_value(Command::InstallPluginUpload {
            stream: crate::StreamId::new(),
            source: UploadSource::Directory,
            name: "example".into(),
            expected_revision: 0,
        })
        .unwrap();
        encoded["data"].as_object_mut().unwrap().remove("source");
        assert!(serde_json::from_value::<Command>(encoded).is_err());
    }

    #[test]
    fn encodes_installation_metadata() {
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
            assert_eq!(encoded["kind"], kind);
            assert_eq!(serde_json::from_value::<Origin>(encoded).unwrap(), origin);
        }
    }
}

#[test]
fn request_preserves_scope() {
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
    assert_eq!(request.id, original.id);
    assert_eq!(request.command, original.command);
    assert_eq!(request.version, 1);
    assert!(request.command.durable());
    let encoded = serde_json::to_value(&request).unwrap();
    assert_eq!(encoded["plugin"]["package"]["name"], "example");
    assert_eq!(serde_json::from_value::<Request>(encoded).unwrap(), request);
    assert!(
        serde_json::to_value(original)
            .unwrap()
            .get("plugin")
            .is_none()
    );
}

#[test]
fn redacts_settings_secrets() {
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
    assert_eq!(request.version, 1);
    assert!(request.command.durable());
    assert!(!format!("{request:?}").contains("private-fixture-token"));
    assert_eq!(
        serde_json::from_slice::<Request>(&serde_json::to_vec(&request).unwrap()).unwrap(),
        request
    );
    assert!(!Command::ReadPluginSettings { package }.durable());
}
