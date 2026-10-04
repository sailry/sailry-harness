use super::*;

fn profile() -> Profile {
    Profile {
        id: RoleId::new(),
        revision: 0,
        key: "review".into(),
        name: "Review".into(),
        appearance: None,
        description: "Review a delegated change".into(),
        model: None,
        max_turns: Some(16),
        skills: Vec::new(),
        instructions: "Check the requested behavior 中文 🙂".into(),
    }
}

#[test]
fn distinguishes_identity_from_key() {
    let mut value = profile();
    let id = value.id;
    for key in ["x", "review", "code-review", "review_2"] {
        value.key = key.into();
        value.validate().unwrap();
        let encoded = serde_json::to_string(&value).unwrap();
        assert_eq!(serde_json::from_str::<Profile>(&encoded).unwrap(), value);
        assert_eq!(value.id, id);
    }
    for key in ["", "X", " x", "x ", "x-", "1x", "x/y", "角色"] {
        value.key = key.into();
        assert!(value.validate().is_err(), "invalid key: {key}");
    }
}

#[test]
fn bounds_configuration() {
    let mut value = profile();
    value.instructions = "x".repeat(16 * 1024);
    value.validate().unwrap();
    value.instructions.push('x');
    assert!(value.validate().is_err());
    value = profile();
    value.name = " ".into();
    assert!(value.validate().is_err());
    value = profile();
    value.description.clear();
    value.validate().unwrap();
    value.description = "x".repeat(1025);
    assert!(value.validate().is_err());
    value = profile();
    value.max_turns = Some(0);
    assert!(value.validate().is_err());
    value.max_turns = None;
    value.skills = vec!["a".into(), "b".into(), "c".into()];
    value.validate().unwrap();
    value.skills.push("d".into());
    assert!(value.validate().is_err());
    value.skills = vec!["a".into(), "a".into()];
    assert!(value.validate().is_err());
    value.skills = vec![String::new()];
    assert!(value.validate().is_err());
}

#[test]
fn appearance() {
    let mut value = profile();
    value.appearance = Some(crate::projects::Appearance {
        icon: "code".into(),
        color: "violet".into(),
    });
    value.validate().unwrap();
    assert_eq!(
        serde_json::from_value::<Profile>(serde_json::to_value(&value).unwrap()).unwrap(),
        value
    );
    value.appearance.as_mut().unwrap().icon = "unknown".into();
    assert!(value.validate().is_err());
    value.appearance.as_mut().unwrap().icon = "code".into();
    value.appearance.as_mut().unwrap().color = "unknown".into();
    assert!(value.validate().is_err());
}

#[test]
fn distinguishes_model_sources() {
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
    assert!(value.validate().is_err());
    assert!(!crate::Command::ListRoles.durable());
    assert!(
        crate::Command::PutRole {
            role: profile(),
            expected_revision: 0
        }
        .durable()
    );
}

mod snapshot {
    use super::*;

    #[test]
    fn rejects_invalid_references() {
        let role = profile();
        let mut snapshot = Snapshot {
            profiles: vec![role.clone()],
            providers: vec![],
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
