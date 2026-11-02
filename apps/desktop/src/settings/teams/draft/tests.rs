use super::*;

mod keys {
    use super::*;

    #[test]
    fn slugs_ascii_names() {
        let taken = BTreeSet::new();
        assert_eq!(key("Front-end Squad", "team", &taken), "front-end-squad");
        assert_eq!(key("  Code Review!  ", "role", &taken), "code-review");
    }

    #[test]
    fn prefixes_unusable_starts() {
        let taken = BTreeSet::new();
        assert_eq!(key("前端小组", "team", &taken), "team");
        assert_eq!(key("2nd pass", "role", &taken), "role-2nd-pass");
    }

    #[test]
    fn appends_free_suffix() {
        let taken = BTreeSet::from(["team".to_owned(), "team-2".to_owned()]);
        assert_eq!(key("前端小组", "team", &taken), "team-3");
    }

    #[test]
    fn generated_keys_are_valid() {
        let taken = BTreeSet::new();
        for name in ["前端小组", "2nd pass", "A_b", "x--y", "Écoute"] {
            assert!(valid_key(&key(name, "role", &taken)), "{name}");
        }
    }

    #[test]
    fn rejects_node_invalid_keys() {
        assert!(valid_key("code-reviewer"));
        assert!(!valid_key(""));
        assert!(!valid_key("Reviewer"));
        assert!(!valid_key("reviewer-"));
        assert!(!valid_key("1reviewer"));
    }
}

mod efforts {
    use super::*;

    #[test]
    fn keeps_supported_choice() {
        let model = crate::settings::Model::example("model");
        assert_eq!(effort(&model, Some(Effort::High)), Some(Effort::High));
    }

    #[test]
    fn falls_back_to_default() {
        let model = crate::settings::Model::example("model");
        assert_eq!(effort(&model, Some(Effort::Max)), Some(Effort::Medium));
        assert_eq!(effort(&model, None), Some(Effort::Medium));
    }

    #[test]
    fn omits_without_reasoning() {
        let mut model = crate::settings::Model::example("model");
        model.reasoning = false;
        assert_eq!(effort(&model, Some(Effort::High)), None);
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
    fn localizes_unchanged_text() {
        let (name, description) = label(&seeded(&role::EXPLORE));
        assert_eq!(name, tr("role_builtin_explore_name"));
        assert_eq!(description, tr("role_builtin_explore_description"));
        let (name, _) = label(&seeded(&role::RESEARCH));
        assert_eq!(name, tr("role_builtin_research_name"));
    }

    #[test]
    fn keeps_edited_text() {
        let mut profile = seeded(&role::EXPLORE);
        profile.name = "Scout".into();
        assert_eq!(label(&profile).0.as_ref(), "Scout");
        let mut other = seeded(&role::EXPLORE);
        other.key = "scout".into();
        assert_eq!(label(&other).0.as_ref(), "Explore");
    }
}
