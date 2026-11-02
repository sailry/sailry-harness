use super::*;
use crate::{Effort, ProviderId, SessionId, role};

fn profile() -> Profile {
    Profile {
        id: TeamId::new(),
        revision: 0,
        key: "review".into(),
        name: "Review".into(),
        appearance: None,
        description: "Review delegated changes".into(),
        model: None,
        instructions: "Coordinate reviewers 中文 🙂".into(),
        members: vec![RoleId::new(), RoleId::new()],
    }
}

#[test]
fn round_trips_profile() {
    let mut value = profile();
    value.appearance = Some(crate::projects::Appearance {
        icon: "code".into(),
        color: "violet".into(),
    });
    value.model = Some(role::Model {
        provider: ProviderId::new(),
        model: "fixture".into(),
        effort: Some(Effort::High),
    });
    value.validate().unwrap();
    let encoded = serde_json::to_value(&value).unwrap();
    assert_eq!(serde_json::from_value::<Profile>(encoded).unwrap(), value);
    assert_eq!(
        value.reference(),
        Reference {
            id: value.id,
            revision: 0
        }
    );
}

#[test]
fn validates_configuration() {
    let mut value = profile();
    for key in ["", "X", "x-", "1x", "团队", DEFAULT] {
        value.key = key.into();
        assert!(value.validate().is_err(), "invalid key: {key}");
    }
    value = profile();
    value.name = " ".into();
    assert!(value.validate().is_err());
    value = profile();
    value.members.push(value.members[0]);
    assert!(value.validate().is_err());
    value.members.clear();
    value.validate().unwrap();
    value.appearance = Some(crate::projects::Appearance {
        icon: "unknown".into(),
        color: "violet".into(),
    });
    assert!(value.validate().is_err());
    value = profile();
    value.model = Some(role::Model {
        provider: ProviderId::new(),
        model: " ".into(),
        effort: None,
    });
    assert!(value.validate().is_err());
}

#[test]
fn classifies_commands() {
    let session = SessionId::new();
    assert!(!crate::Command::ListTeams.durable());
    for command in [
        crate::Command::PutTeam {
            team: profile(),
            expected_revision: 0,
        },
        crate::Command::RemoveTeam {
            team: TeamId::new(),
            expected_revision: 1,
        },
        crate::Command::SetSessionExecution {
            execution: crate::conversation::Execution::Team,
            models: Default::default(),
            session,
            expected_revision: 1,
            team: Some(profile().reference().into()),
        },
        crate::Command::SetSessionMemberModel {
            session,
            expected_revision: 1,
            role: RoleId::new(),
            model: None,
        },
    ] {
        assert!(command.durable());
        let encoded = serde_json::to_value(&command).unwrap();
        assert_eq!(
            serde_json::from_value::<crate::Command>(encoded).unwrap(),
            command
        );
    }
    let encoded = serde_json::to_value(crate::Command::SetSessionExecution {
        execution: crate::conversation::Execution::Team,
        models: Default::default(),
        session,
        expected_revision: 1,
        team: None,
    })
    .unwrap();
    assert_eq!(encoded["kind"], "set_session_team");
    assert!(encoded["data"]["team"].is_null());
}

#[test]
fn preserves_stored_selection_commands() {
    let session = SessionId::new();
    let team = TeamId::new();
    let stored = format!(
        r#"{{"kind":"set_session_team","data":{{"session":"{session}","expected_revision":1,"team":{{"id":"{team}","revision":2}}}}}}"#
    );
    let command: crate::Command = serde_json::from_str(&stored).unwrap();
    assert!(matches!(command, crate::Command::SetSessionExecution {
        execution: crate::conversation::Execution::Team, ref models, ..
    } if models.is_empty()));
    // Existing approval digests and request identities keep their original bytes.
    assert_eq!(serde_json::to_string(&command).unwrap(), stored);
}

mod settings {
    use super::*;

    #[test]
    fn defaults_to_eight_children() {
        let value = Settings::default();
        assert_eq!((value.revision, value.child_concurrency), (0, 8));
        value.validate().unwrap();
        let encoded = serde_json::to_value(&value).unwrap();
        assert_eq!(
            encoded,
            serde_json::json!({"revision":0,"child_concurrency":8})
        );
        assert_eq!(serde_json::from_value::<Settings>(encoded).unwrap(), value);
    }

    #[test]
    fn requires_a_positive_budget() {
        let mut value = Settings {
            revision: 0,
            child_concurrency: 0,
        };
        assert!(value.validate().is_err());
        value.child_concurrency = u64::MAX;
        value.validate().unwrap();
        assert!(
            serde_json::from_value::<Settings>(
                serde_json::json!({"revision":0,"child_concurrency":8,"other":true})
            )
            .is_err()
        );
    }

    #[test]
    fn classifies_commands() {
        assert!(!crate::Command::ReadTeamSettings.durable());
        let command = crate::Command::SaveTeamSettings(Settings::default());
        assert!(command.durable());
        let encoded = serde_json::to_value(&command).unwrap();
        assert_eq!(encoded["kind"], "save_team_settings");
        assert_eq!(
            serde_json::from_value::<crate::Command>(encoded).unwrap(),
            command
        );
    }
}

mod snapshot {
    use super::*;

    fn member() -> role::Profile {
        role::Profile {
            id: RoleId::new(),
            revision: 1,
            key: "review".into(),
            name: "Review".into(),
            appearance: None,
            description: String::new(),
            read_only: false,
            model: None,
            max_turns: None,
            skills: vec![],
            tools: None,
            instructions: String::new(),
        }
    }

    #[test]
    fn omits_default_team() {
        let snapshot = role::Snapshot {
            profiles: vec![member()],
            ..Default::default()
        };
        let encoded = serde_json::to_value(&snapshot).unwrap();
        for field in ["team", "instructions", "models"] {
            assert!(encoded.get(field).is_none(), "{field}");
        }
        assert_eq!(
            serde_json::from_value::<role::Snapshot>(encoded).unwrap(),
            snapshot
        );
    }

    #[test]
    fn prefers_member_override() {
        let mut role = member();
        let configured = role::Model {
            provider: ProviderId::new(),
            model: "configured".into(),
            effort: None,
        };
        let mut snapshot = role::Snapshot {
            profiles: vec![role.clone()],
            team: Some(profile().reference()),
            instructions: "Lead".into(),
            ..Default::default()
        };
        assert_eq!(snapshot.model(&role), None);
        role.model = Some(configured.clone());
        snapshot.profiles[0] = role.clone();
        assert_eq!(snapshot.model(&role), Some(&configured));
        let chosen = role::Model {
            provider: configured.provider,
            model: "chosen".into(),
            effort: Some(Effort::Low),
        };
        snapshot.models.insert(role.id, chosen.clone());
        assert_eq!(snapshot.model(&role), Some(&chosen));
        let encoded = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(
            serde_json::from_value::<role::Snapshot>(encoded).unwrap(),
            snapshot
        );
    }

    #[test]
    fn requires_override_providers() {
        let role = member();
        let mut snapshot = role::Snapshot {
            profiles: vec![role.clone()],
            ..Default::default()
        };
        snapshot.models.insert(
            role.id,
            role::Model {
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: None,
            },
        );
        assert!(snapshot.validate().is_err());
        snapshot.models.clear();
        snapshot.models.insert(
            RoleId::new(),
            role::Model {
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: None,
            },
        );
        assert!(snapshot.validate().is_err());
    }
}
