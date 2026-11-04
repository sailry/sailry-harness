use super::*;

mod keys {
    use super::*;

    #[test]
    fn generation_and_validation() {
        for (case, name, prefix, taken, expected) in [
            (
                "ASCII slug",
                "Front-end Squad",
                "team",
                BTreeSet::new(),
                "front-end-squad",
            ),
            (
                "trim punctuation",
                "  Code Review!  ",
                "role",
                BTreeSet::new(),
                "code-review",
            ),
            (
                "Unicode fallback",
                "前端小组",
                "team",
                BTreeSet::new(),
                "team",
            ),
            (
                "numeric prefix",
                "2nd pass",
                "role",
                BTreeSet::new(),
                "role-2nd-pass",
            ),
            (
                "free suffix",
                "前端小组",
                "team",
                BTreeSet::from(["team".into(), "team-2".into()]),
                "team-3",
            ),
        ] {
            assert_eq!(key(name, prefix, &taken), expected, "{case}");
        }
        let taken = BTreeSet::new();
        for name in ["前端小组", "2nd pass", "A_b", "x--y", "Écoute"] {
            assert!(
                valid_key(&key(name, "role", &taken)),
                "generated key: {name}"
            );
        }
        for (value, expected) in [
            ("code-reviewer", true),
            ("", false),
            ("Reviewer", false),
            ("reviewer-", false),
            ("1reviewer", false),
        ] {
            assert_eq!(valid_key(value), expected, "key validation: {value:?}");
        }
    }
}

mod efforts {
    use super::*;

    #[test]
    fn supported_and_default_choices() {
        let mut model = crate::settings::Model::example("model");
        for (case, reasoning, selected, expected) in [
            ("supported", true, Some(Effort::High), Some(Effort::High)),
            ("unsupported", true, Some(Effort::Max), Some(Effort::Medium)),
            ("default", true, None, Some(Effort::Medium)),
            ("no reasoning", false, Some(Effort::High), None),
        ] {
            model.reasoning = reasoning;
            assert_eq!(effort(&model, selected), expected, "{case}");
        }
    }
}

#[test]
fn research_preset_keeps_plugin_template() {
    let preset = Preset::Research;
    assert!(preset.read_only());
    assert_eq!(preset.max_turns(), Some(8));
    assert_eq!(preset.skills(), ["web-search:research"]);
    assert!(valid_key(preset.key()));
}

mod builtin_labels {
    use super::*;

    fn seeded(builtin: &role::Builtin) -> role::Profile {
        role::Profile {
            key: builtin.key.into(),
            name: builtin.name.into(),
            description: builtin.description.into(),
            ..crate::settings::teams::form::blank(sailry_protocol::RoleId::new())
        }
    }

    #[test]
    fn declared_and_edited_text() {
        let mut profile = seeded(&role::EXPLORE);
        let (name, description) = label(&profile);
        assert_eq!(name, tr("role_builtin_explore_name"), "unchanged name");
        assert_eq!(
            description,
            tr("role_builtin_explore_description"),
            "unchanged description"
        );
        let (name, _) = label(&seeded(&role::RESEARCH));
        assert_eq!(name, tr("role_builtin_research_name"), "research name");
        profile.name = "Scout".into();
        assert_eq!(label(&profile).0.as_ref(), "Scout", "edited name");
        profile = seeded(&role::EXPLORE);
        profile.key = "scout".into();
        assert_eq!(label(&profile).0.as_ref(), "Explore", "custom role key");
    }
}
