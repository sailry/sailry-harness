use super::*;
use core::prelude::v1::test;

#[test]
fn orders_explicit_levels_and_budgets() {
    let mut choices = [Effort::High, Effort::Disabled, Effort::Max, Effort::Low];
    choices.sort_by_key(|effort| order(*effort));
    assert_eq!(
        choices,
        [Effort::Disabled, Effort::Low, Effort::High, Effort::Max]
    );
    assert!(order(Effort::Budget(1024)) < order(Effort::Budget(8192)));
}

mod feature_choices {
    use super::*;
    use sailry_protocol::conversation::features::{Feature, FeatureOption};
    use std::collections::BTreeMap;

    fn declarations() -> Vec<Feature> {
        vec![
            Feature {
                id: "speed".into(),
                name: Some("Speed".into()),
                icon: None,
                options: vec![
                    FeatureOption {
                        id: "fast".into(),
                        name: Some("Fast".into()),
                    },
                    FeatureOption {
                        id: "future-choice".into(),
                        name: None,
                    },
                ],
            },
            Feature {
                id: "format".into(),
                name: None,
                icon: None,
                options: vec![FeatureOption {
                    id: "compact".into(),
                    name: Some("Compact".into()),
                }],
            },
        ]
    }

    #[test]
    fn removes_unadvertised_choices() {
        let selected = BTreeMap::from([
            ("speed".into(), "future-choice".into()),
            ("format".into(), "missing".into()),
            ("removed".into(), "value".into()),
        ]);
        assert_eq!(
            super::super::features::retain(&declarations(), &selected),
            BTreeMap::from([("speed".into(), "future-choice".into())])
        );
        assert!(super::super::features::retain(&[], &selected).is_empty());
    }

    #[test]
    fn labels_follow_declared_order() {
        let labels = |selected: &BTreeMap<_, _>| {
            super::super::features::selected(&declarations(), selected)
                .into_iter()
                .map(|choice| choice.label)
                .collect::<Vec<_>>()
        };
        let selected = BTreeMap::from([
            ("speed".into(), "fast".into()),
            ("format".into(), "compact".into()),
        ]);
        assert_eq!(
            labels(&selected),
            vec![tr("composer_feature_fast"), SharedString::from("Compact")]
        );
        let mut selected = selected;
        selected.insert("speed".into(), "future-choice".into());
        assert_eq!(selected.len(), 2);
        assert_eq!(labels(&selected)[0], "future-choice");
        selected.remove("speed");
        assert_eq!(labels(&selected), vec![SharedString::from("Compact")]);
    }

    #[test]
    fn fast_glyph_uses_the_declared_choice() {
        let mut features = declarations();
        let selected = BTreeMap::from([("speed".into(), "fast".into())]);
        assert!(super::super::features::selected(&features, &selected)[0].fast);
        assert!(
            super::super::features::selected(&features, &selected)[0]
                .icon
                .is_some()
        );
        features[0].id = "custom-speed".into();
        let custom = BTreeMap::from([("custom-speed".into(), "fast".into())]);
        assert!(!super::super::features::selected(&features, &custom)[0].fast);
        assert!(
            super::super::features::selected(&features, &custom)[0]
                .icon
                .is_none()
        );
        assert!(super::super::features::selected(&features, &selected).is_empty());
    }
}

mod teams {
    use super::team::{Chip, Draft, Roster, Step, Team, effective};
    use super::*;
    use sailry_protocol::{ProviderId, RoleId, TeamId, conversation::Execution, role, team};
    use std::collections::BTreeMap;

    fn profile(name: &str, model: Option<role::Model>) -> role::Profile {
        role::Profile {
            id: RoleId::new(),
            revision: 1,
            key: name.into(),
            name: name.into(),
            appearance: None,
            description: String::new(),
            read_only: false,
            model,
            max_turns: None,
            skills: Vec::new(),
            tools: None,
            instructions: String::new(),
        }
    }

    fn model(name: &str) -> role::Model {
        role::Model {
            provider: ProviderId::new(),
            model: name.into(),
            effort: Some(Effort::High),
        }
    }

    fn stored(members: Vec<RoleId>) -> team::Profile {
        team::Profile {
            id: TeamId::new(),
            revision: 3,
            key: "frontend".into(),
            name: "Frontend".into(),
            appearance: None,
            description: String::new(),
            model: None,
            instructions: String::new(),
            members,
        }
    }

    mod chip {
        use super::*;

        #[test]
        fn model_execution_is_distinct_from_default_team() {
            let roster = Roster {
                execution: Execution::Model,
                ..Default::default()
            };
            assert_eq!(roster.chip(), Chip::Model);
            assert!(!roster.selected(None));
            assert!(roster.changes(None));
        }

        #[test]
        fn default_team_keeps_model_content() {
            let roles = [profile("reviewer", None)];
            let overrides = BTreeMap::from([(roles[0].id, model("fast"))]);
            let roster = Roster {
                members: Roster::members(&roles, &overrides),
                ..Default::default()
            };
            assert_eq!(roster.chip(), Chip::Model);
        }

        #[test]
        fn other_team_shows_team_and_override_dot() {
            let roles = [profile("reviewer", None), profile("tester", None)];
            let team = Team::from(&stored(roles.iter().map(|role| role.id).collect()));
            let mut roster = Roster {
                team: Some(team.clone()),
                members: Roster::members(&roles, &BTreeMap::new()),
                ..Default::default()
            };
            assert_eq!(
                roster.chip(),
                Chip::Team {
                    team: &team,
                    overridden: false
                }
            );
            roster.members = Roster::members(&roles, &BTreeMap::from([(roles[1].id, model("x"))]));
            assert_eq!(
                roster.chip(),
                Chip::Team {
                    team: &team,
                    overridden: true
                }
            );
        }

        #[test]
        fn newer_revision_changes_current_team() {
            let profile = stored(Vec::new());
            let roster = Roster {
                team: Some(Team::from(&profile)),
                ..Default::default()
            };
            assert!(!roster.changes(Some(&Team::from(&profile))));
            let newer = team::Profile {
                revision: profile.revision + 1,
                ..profile
            };
            assert!(roster.changes(Some(&Team::from(&newer))));
            assert!(roster.changes(None));
        }

        #[test]
        fn default_recaptures_catalog_membership() {
            let roles = [profile("reviewer", None), profile("tester", None)];
            let mut roster = Roster {
                members: Roster::members(&roles[..1], &BTreeMap::new()),
                catalog: roles.iter().map(role::Profile::reference).collect(),
                ..Default::default()
            };
            assert!(roster.changes(None));
            roster.members = Roster::members(&roles, &BTreeMap::new());
            assert!(!roster.changes(None));
            roster.catalog.pop();
            assert!(roster.changes(None));
        }

        #[test]
        fn same_team_recaptures_role_edits() {
            let roles = [profile("reviewer", None), profile("tester", None)];
            let team = Team::from(&stored(vec![roles[0].id]));
            let mut roster = Roster {
                team: Some(team.clone()),
                members: Roster::members(&roles[..1], &BTreeMap::new()),
                catalog: roles.iter().map(role::Profile::reference).collect(),
                ..Default::default()
            };
            assert!(!roster.changes(Some(&team)));
            // An unrelated role edit does not reset the selected team's overrides.
            roster.catalog[1].revision += 1;
            assert!(!roster.changes(Some(&team)));
            roster.catalog[0].revision += 1;
            assert!(roster.changes(Some(&team)));
        }

        #[test]
        fn unchanged_selection_preserves_overrides() {
            let roles = [profile("reviewer", None)];
            let overrides = BTreeMap::from([(roles[0].id, model("custom"))]);
            let roster = Roster {
                members: Roster::members(&roles, &overrides),
                catalog: roles.iter().map(role::Profile::reference).collect(),
                ..Default::default()
            };
            assert!(roster.members[0].overridden);
            assert!(!roster.changes(None));
        }
    }

    #[test]
    fn member_model_prefers_override_then_role() {
        let fixed = profile("reviewer", Some(model("role")));
        let follows = profile("tester", None);
        let overrides = BTreeMap::from([(fixed.id, model("override"))]);
        assert_eq!(effective(&overrides, &fixed).unwrap().model, "override");
        assert_eq!(effective(&BTreeMap::new(), &fixed).unwrap().model, "role");
        assert!(effective(&overrides, &follows).is_none());
        let snapshot = role::Snapshot {
            profiles: vec![fixed.clone(), follows.clone()],
            models: overrides.clone(),
            ..Default::default()
        };
        for profile in [&fixed, &follows] {
            assert_eq!(snapshot.model(profile), effective(&overrides, profile));
        }
    }

    mod start {
        use super::*;

        #[test]
        fn team_selection_replaces_model_execution() {
            let session = SessionId::new();
            let captured = role::Snapshot {
                execution: Execution::Model,
                ..Default::default()
            };
            assert_eq!(
                Step::Team(None).command(session, 4, &captured),
                Some(Command::SetSessionExecution {
                    session,
                    expected_revision: 4,
                    execution: Execution::Team,
                    team: None,
                    models: BTreeMap::new(),
                })
            );
        }

        #[test]
        fn default_draft_sends_nothing() {
            assert!(Draft::default().steps(&[stored(Vec::new())]).is_empty());
        }

        #[test]
        fn sends_current_team_revision_then_members() {
            let roles = [profile("reviewer", None)];
            let catalog = stored(vec![roles[0].id]);
            let fast = model("fast");
            let draft = Draft {
                team: Some(catalog.id),
                models: BTreeMap::from([(roles[0].id, fast.clone())]),
            };
            assert_eq!(
                draft.steps(std::slice::from_ref(&catalog)),
                vec![
                    Step::Team(Some(catalog.reference())),
                    Step::Member(roles[0].id, fast.clone()),
                ]
            );
            // A removed team leaves the Default team in place.
            assert_eq!(draft.steps(&[]), vec![Step::Member(roles[0].id, fast)]);
        }

        #[test]
        fn builds_revision_checked_commands() {
            let session = SessionId::new();
            let roles = [profile("reviewer", None)];
            let catalog = stored(vec![roles[0].id]);
            let captured = role::Snapshot {
                profiles: roles.to_vec(),
                ..Default::default()
            };
            assert_eq!(
                Step::Team(Some(catalog.reference())).command(session, 4, &captured),
                Some(Command::SetSessionExecution {
                    session,
                    expected_revision: 4,
                    execution: Execution::Team,
                    team: Some(catalog.reference().into()),
                    models: BTreeMap::new(),
                })
            );
            let chosen = model("fast");
            assert_eq!(
                Step::Member(roles[0].id, chosen.clone()).command(session, 5, &captured),
                Some(Command::SetSessionMemberModel {
                    session,
                    expected_revision: 5,
                    role: roles[0].id,
                    model: Some(chosen.clone()),
                })
            );
        }

        #[test]
        fn skips_applied_or_missing_members() {
            let session = SessionId::new();
            let roles = [profile("reviewer", None)];
            let chosen = model("fast");
            let applied = role::Snapshot {
                profiles: roles.to_vec(),
                models: BTreeMap::from([(roles[0].id, chosen.clone())]),
                ..Default::default()
            };
            assert!(Step::Team(None).command(session, 1, &applied).is_none());
            assert!(
                Step::Member(roles[0].id, chosen.clone())
                    .command(session, 1, &applied)
                    .is_none()
            );
            assert!(
                Step::Member(RoleId::new(), chosen)
                    .command(session, 1, &applied)
                    .is_none()
            );
        }
    }
}
