use super::*;

fn profile() -> Profile {
    Profile {
        read_only: false,
        id: RoleId::new(),
        revision: 0,
        key: "review".into(),
        name: "Review".into(),
        appearance: None,
        description: "Review a delegated change".into(),
        model: None,
        max_turns: Some(16),
        skills: Vec::new(),
        tools: None,
        instructions: "Check the requested behavior 中文 🙂".into(),
    }
}

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "distinguishes_identity_from_key",
            (|scenario: &'static str| {
                let mut value = profile();
                let id = value.id;
                for key in ["x", "review", "code-review", "review_2"] {
                    value.key = key.into();
                    value.validate().unwrap();
                    let encoded = serde_json::to_string(&value).unwrap();
                    assert_eq!(
                        serde_json::from_str::<Profile>(&encoded).unwrap(),
                        value,
                        "{scenario}"
                    );
                    assert_eq!(value.id, id, "{scenario}");
                }
                for key in ["", "X", " x", "x ", "x-", "1x", "x/y", "角色"] {
                    value.key = key.into();
                    assert!(value.validate().is_err(), "{scenario}: invalid key: {key}");
                }
            }) as fn(&'static str),
        ),
        (
            "preserves_execution_constraint",
            (|scenario: &'static str| {
                let mut value = profile();
                value.read_only = true;
                value.validate().unwrap();
                let mut encoded = serde_json::to_value(&value).unwrap();
                assert_eq!(encoded["read_only"], true, "{scenario}");
                assert_eq!(
                    serde_json::from_value::<Profile>(encoded.clone()).unwrap(),
                    value,
                    "{scenario}"
                );
                encoded.as_object_mut().unwrap().remove("read_only");
                assert!(
                    serde_json::from_value::<Profile>(encoded).is_err(),
                    "{scenario}"
                );
            }),
        ),
        (
            "validates_configuration",
            (|scenario: &'static str| {
                let mut value = profile();
                value.instructions = "x".repeat(16 * 1024);
                value.validate().unwrap();
                value.instructions.push('x');
                value.validate().unwrap();
                value = profile();
                value.name = " ".into();
                assert!(value.validate().is_err(), "{scenario}");
                value = profile();
                value.description.clear();
                value.validate().unwrap();
                value.description = "x".repeat(1025);
                value.validate().unwrap();
                value = profile();
                value.max_turns = Some(0);
                assert!(value.validate().is_err(), "{scenario}");
                value.max_turns = None;
                value.skills = vec!["a".into(), "b".into(), "c".into()];
                value.validate().unwrap();
                value.skills.push("d".into());
                value.validate().unwrap();
                value.skills = vec!["a".into(), "a".into()];
                assert!(value.validate().is_err(), "{scenario}");
                value.skills = vec![String::new()];
                assert!(value.validate().is_err(), "{scenario}");
            }),
        ),
        (
            "validates_tool_selections",
            (|scenario: &'static str| {
                use crate::plugin::conversation::Tool;
                let mut value = profile();
                value.tools = Some(vec![
                    Tool::Package {
                        package: "files".into(),
                        name: "read_file".into(),
                    },
                    Tool::Package {
                        package: "commands".into(),
                        name: "read_command".into(),
                    },
                ]);
                value.validate().unwrap();
                let encoded = serde_json::to_value(&value).unwrap();
                assert_eq!(
                    serde_json::from_value::<Profile>(encoded).unwrap(),
                    value,
                    "{scenario}"
                );
                let duplicate = value.tools.as_ref().unwrap()[0].clone();
                value.tools.as_mut().unwrap().push(duplicate);
                assert!(value.validate().is_err(), "{scenario}");
                value.tools = Some(vec![Tool::Plugin {
                    name: "read".into(),
                    server: None,
                }]);
                assert!(value.validate().is_err(), "{scenario}");
                value.tools = Some(vec![]);
                value.validate().unwrap();
                let mut encoded = serde_json::to_value(profile()).unwrap();
                encoded.as_object_mut().unwrap().remove("tools");
                assert!(
                    serde_json::from_value::<Profile>(encoded)
                        .unwrap()
                        .tools
                        .is_none(),
                    "{scenario}"
                );
            }),
        ),
        (
            "appearance",
            (|scenario: &'static str| {
                let mut value = profile();
                value.appearance = Some(crate::projects::Appearance {
                    icon: "code".into(),
                    color: "violet".into(),
                });
                value.validate().unwrap();
                assert_eq!(
                    serde_json::from_value::<Profile>(serde_json::to_value(&value).unwrap())
                        .unwrap(),
                    value,
                    "{scenario}"
                );
                value.appearance.as_mut().unwrap().icon = "unknown".into();
                assert!(value.validate().is_err(), "{scenario}");
                value.appearance.as_mut().unwrap().icon = "code".into();
                value.appearance.as_mut().unwrap().color = "unknown".into();
                assert!(value.validate().is_err(), "{scenario}");
            }),
        ),
        (
            "distinguishes_model_sources",
            (|scenario: &'static str| {
                let mut value = profile();
                value.model = Some(Model {
                    provider: ProviderId::new(),
                    model: "fixture".into(),
                    effort: None,
                });
                value.validate().unwrap();
                value.model.as_mut().unwrap().effort = Some(Effort::High);
                value.validate().unwrap();
                value.model.as_mut().unwrap().model.clear();
                assert!(value.validate().is_err(), "{scenario}");
                assert!(!crate::Command::ListRoles.durable(), "{scenario}");
                assert!(
                    crate::Command::PutRole {
                        role: profile(),
                        expected_revision: 0
                    }
                    .durable(),
                    "{scenario}"
                );
            }),
        ),
        (
            "builtins_have_visible_appearances",
            (|scenario: &'static str| {
                for builtin in [&EXPLORE, &RESEARCH] {
                    assert_eq!(
                        Builtin::find(builtin.key).unwrap().name,
                        builtin.name,
                        "{scenario}"
                    );
                    let mut value = profile();
                    value.appearance = Some(builtin.appearance());
                    value.validate().unwrap();
                    assert_ne!(builtin.color, "none", "{scenario}");
                }
                assert!(Builtin::find("review").is_none(), "{scenario}");
                for _ in 0..64 {
                    let mut value = profile();
                    let appearance = crate::projects::Appearance::random();
                    assert_ne!(appearance.color, "none", "{scenario}");
                    value.appearance = Some(appearance);
                    value.validate().unwrap();
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

mod snapshot {
    use super::*;

    #[test]
    fn rejects_invalid_references() {
        let role = profile();
        let mut snapshot = Snapshot {
            profiles: vec![role.clone()],
            providers: vec![],
            ..Default::default()
        };
        snapshot.validate().unwrap();
        assert_eq!(
            serde_json::from_value::<Snapshot>(serde_json::to_value(&snapshot).unwrap()).unwrap(),
            snapshot
        );
        snapshot.profiles.push(role);
        assert!(snapshot.validate().is_err());
        snapshot.profiles[1].id = RoleId::new();
        assert!(snapshot.validate().is_err());
        snapshot.profiles[1].key = "another".into();
        snapshot.validate().unwrap();
        snapshot.profiles[1].model = Some(Model {
            provider: ProviderId::new(),
            model: "fixture".into(),
            effort: None,
        });
        assert!(snapshot.validate().is_err());
        assert!(serde_json::from_str::<Snapshot>(r#"{"profiles":[]}"#).is_err());
        assert!(
            crate::Command::SetSessionRoles {
                session: crate::SessionId::new(),
                expected_revision: 1,
                roles: vec![]
            }
            .durable()
        );
    }
}
