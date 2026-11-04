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
    fn declared_selection() {
        let mut features = declarations();
        let selected = BTreeMap::from([
            ("speed".into(), "future-choice".into()),
            ("format".into(), "missing".into()),
            ("removed".into(), "value".into()),
        ]);
        assert_eq!(
            super::super::features::retain(&features, &selected),
            BTreeMap::from([("speed".into(), "future-choice".into())]),
            "retained choices"
        );
        assert!(
            super::super::features::retain(&[], &selected).is_empty(),
            "missing declarations"
        );
        for (case, selected, expected) in [
            (
                "declared order",
                BTreeMap::from([
                    ("speed".into(), "fast".into()),
                    ("format".into(), "compact".into()),
                ]),
                vec![tr("composer_feature_fast"), SharedString::from("Compact")],
            ),
            (
                "future choice",
                BTreeMap::from([
                    ("speed".into(), "future-choice".into()),
                    ("format".into(), "compact".into()),
                ]),
                vec![
                    SharedString::from("future-choice"),
                    SharedString::from("Compact"),
                ],
            ),
            (
                "removed choice",
                BTreeMap::from([("format".into(), "compact".into())]),
                vec![SharedString::from("Compact")],
            ),
        ] {
            let labels = super::super::features::selected(&features, &selected)
                .into_iter()
                .map(|choice| choice.label)
                .collect::<Vec<_>>();
            assert_eq!(labels, expected, "{case}: labels");
            if case == "future choice" {
                assert_eq!(selected.len(), 2, "{case}: selection count");
            }
        }
        let selected = BTreeMap::from([("speed".into(), "fast".into())]);
        let choices = super::super::features::selected(&features, &selected);
        assert!(choices[0].fast, "fast glyph: recognized choice");
        assert!(choices[0].icon.is_some(), "fast glyph: icon");
        features[0].id = "custom-speed".into();
        let custom = BTreeMap::from([("custom-speed".into(), "fast".into())]);
        let choices = super::super::features::selected(&features, &custom);
        assert!(!choices[0].fast, "custom glyph: not fast");
        assert!(choices[0].icon.is_none(), "custom glyph: no default icon");
        assert!(
            super::super::features::selected(&features, &selected).is_empty(),
            "removed feature"
        );
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
        fn execution_and_overrides() {
            let roles = [profile("reviewer", None), profile("tester", None)];
            let roster = Roster {
                execution: Execution::Model,
                ..Default::default()
            };
            assert_eq!(roster.chip(), Chip::Model, "single model: chip");
            assert!(
                !roster.selected(None),
                "single model: no default team selection"
            );
            assert!(
                roster.changes(None),
                "single model: team selection changes execution"
            );
            let overrides = BTreeMap::from([(roles[0].id, model("fast"))]);
            let roster = Roster {
                members: Roster::members(&roles[..1], &overrides, &[]),
                ..Default::default()
            };
            assert_eq!(roster.chip(), Chip::Model, "default team: model content");
            let team = Team::from(&stored(roles.iter().map(|role| role.id).collect()));
            let mut roster = Roster {
                team: Some(team.clone()),
                members: Roster::members(&roles, &BTreeMap::new(), &[]),
                ..Default::default()
            };
            for (case, overrides, overridden) in [
                ("captured", BTreeMap::new(), false),
                (
                    "override",
                    BTreeMap::from([(roles[1].id, model("x"))]),
                    true,
                ),
            ] {
                roster.members = Roster::members(&roles, &overrides, &[]);
                assert_eq!(
                    roster.chip(),
                    Chip::Team {
                        team: &team,
                        overridden
                    },
                    "{case}"
                );
            }
        }

        #[test]
        fn catalog_revisions_and_membership() {
            let roles = [profile("reviewer", None), profile("tester", None)];
            let profile = stored(Vec::new());
            let roster = Roster {
                team: Some(Team::from(&profile)),
                ..Default::default()
            };
            let newer = team::Profile {
                revision: profile.revision + 1,
                ..profile.clone()
            };
            for (case, desired, expected) in [
                ("unchanged team", Some(Team::from(&profile)), false),
                ("newer team", Some(Team::from(&newer)), true),
                ("default team", None, true),
            ] {
                assert_eq!(roster.changes(desired.as_ref()), expected, "{case}");
            }
            let mut roster = Roster {
                members: Roster::members(&roles[..1], &BTreeMap::new(), &[]),
                catalog: roles.iter().map(role::Profile::reference).collect(),
                ..Default::default()
            };
            assert!(roster.changes(None), "default team: changed membership");
            roster.members = Roster::members(&roles, &BTreeMap::new(), &[]);
            assert!(!roster.changes(None), "default team: captured membership");
            roster.catalog.pop();
            assert!(roster.changes(None), "default team: removed catalog role");

            let team = Team::from(&stored(vec![roles[0].id]));
            let mut roster = Roster {
                team: Some(team.clone()),
                members: Roster::members(&roles[..1], &BTreeMap::new(), &[]),
                catalog: roles.iter().map(role::Profile::reference).collect(),
                ..Default::default()
            };
            assert!(!roster.changes(Some(&team)), "named team: unchanged role");
            roster.catalog[1].revision += 1;
            assert!(
                !roster.changes(Some(&team)),
                "named team: unrelated role edit"
            );
            roster.catalog[0].revision += 1;
            assert!(roster.changes(Some(&team)), "named team: member edit");
            let overrides = BTreeMap::from([(roles[0].id, model("custom"))]);
            let roster = Roster {
                members: Roster::members(&roles[..1], &overrides, &[]),
                catalog: roles[..1].iter().map(role::Profile::reference).collect(),
                ..Default::default()
            };
            assert!(
                roster.members[0].overridden,
                "unchanged selection: override retained"
            );
            assert!(!roster.changes(None), "unchanged selection: no recapture");
        }
    }

    #[test]
    fn appearances_follow_catalog_identity_only() {
        use sailry_protocol::projects::Appearance;

        let captured = profile("reviewer", Some(model("captured")));
        let removed = role::Profile {
            appearance: Some(Appearance {
                icon: "code".into(),
                color: "blue".into(),
            }),
            ..profile("removed", None)
        };
        let current = role::Profile {
            revision: captured.revision + 1,
            appearance: Some(Appearance {
                icon: "science".into(),
                color: "green".into(),
            }),
            model: Some(model("current")),
            ..captured.clone()
        };
        let replacement = role::Profile {
            id: RoleId::new(),
            appearance: Some(Appearance {
                icon: "ai".into(),
                color: "cyan".into(),
            }),
            ..removed.clone()
        };
        let overrides = BTreeMap::from([(captured.id, model("override"))]);
        let members = Roster::members(
            [&captured, &removed],
            &overrides,
            &[current.clone(), replacement],
        );
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].appearance, current.appearance);
        assert_eq!(members[0].revision, captured.revision);
        assert!(members[0].overridden);
        assert_eq!(members[1].appearance, removed.appearance);
        assert_eq!(captured.model.as_ref().unwrap().model, "captured");
        assert_eq!(effective(&overrides, &captured).unwrap().model, "override");

        let neutral = role::Profile {
            appearance: None,
            ..current
        };
        assert!(
            Roster::members([&captured], &overrides, &[neutral])[0]
                .appearance
                .is_none()
        );
        assert_eq!(
            Roster::members([&removed], &overrides, &[])[0].appearance,
            removed.appearance
        );
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
        fn revision_checked_selection_commands() {
            let session = SessionId::new();
            let roles = [profile("reviewer", None)];
            let catalog = stored(vec![roles[0].id]);
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
                    models: BTreeMap::new()
                }),
                "replace model execution"
            );
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
                    models: BTreeMap::new()
                }),
                "team revision"
            );
            let chosen = model("fast");
            assert_eq!(
                Step::Member(roles[0].id, chosen.clone()).command(session, 5, &captured),
                Some(Command::SetSessionMemberModel {
                    session,
                    expected_revision: 5,
                    role: roles[0].id,
                    model: Some(chosen.clone())
                }),
                "member revision"
            );
            let applied = role::Snapshot {
                profiles: roles.to_vec(),
                models: BTreeMap::from([(roles[0].id, chosen.clone())]),
                ..Default::default()
            };
            assert!(
                Step::Team(None).command(session, 1, &applied).is_none(),
                "default already applied"
            );
            assert!(
                Step::Member(roles[0].id, chosen.clone())
                    .command(session, 1, &applied)
                    .is_none(),
                "member already applied"
            );
            assert!(
                Step::Member(RoleId::new(), chosen)
                    .command(session, 1, &applied)
                    .is_none(),
                "member removed"
            );
        }

        #[test]
        fn draft_selection_and_members() {
            let roles = [profile("reviewer", None)];
            let catalog = stored(vec![roles[0].id]);
            assert!(
                Draft::default().steps(&[stored(Vec::new())]).is_empty(),
                "default draft"
            );
            let fast = model("fast");
            let draft = Draft {
                team: Some(catalog.id),
                models: BTreeMap::from([(roles[0].id, fast.clone())]),
            };
            assert_eq!(
                draft.steps(std::slice::from_ref(&catalog)),
                vec![
                    Step::Team(Some(catalog.reference())),
                    Step::Member(roles[0].id, fast.clone())
                ],
                "current revision and members"
            );
            assert_eq!(
                draft.steps(&[]),
                vec![Step::Member(roles[0].id, fast)],
                "removed team"
            );
        }
    }
}
