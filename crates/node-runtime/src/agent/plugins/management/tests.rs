use super::*;

#[test]
fn preserves_anthropic_branches() {
    use adk_core::SchemaAdapter;
    for plan in [false, true] {
        let schema = Management { plan }.parameters();
        let wire = adk_model::anthropic::AnthropicSchemaAdapter.normalize_schema(schema.clone());
        assert!(wire.get("oneOf").is_none());
        assert_eq!(wire["properties"], schema["properties"]);
        let canonical = jsonschema::draft202012::options().build(&schema).unwrap();
        let projected = jsonschema::draft202012::options().build(&wire).unwrap();
        for (arguments, valid) in [
            (json!({"action":"list"}), true),
            (json!({"action":"search","source":"official"}), true),
            (
                json!({"action":"uninstall","name":"fixture","expected_revision":1}),
                !plan,
            ),
            (json!({"action":"search"}), false),
            (json!({"action":"uninstall","name":"fixture"}), false),
            (json!({"action":"list","name":"fixture"}), false),
            (
                json!({"action":"inspect","target":{"kind":"installed"}}),
                false,
            ),
        ] {
            assert_eq!(canonical.is_valid(&arguments), valid, "{arguments}");
            assert_eq!(projected.is_valid(&arguments), valid, "{arguments}");
        }
        assert!(schema.get("oneOf").is_some());
    }
}

fn info(version: &str, origin: plugin::Origin) -> plugin::Info {
    plugin::Info {
        release_notes: None,
        summary: plugin::Summary {
            name: "fixture".into(),
            revision: 7,
            digest: "frozen".into(),
            settings_revision: 3,
            enabled: false,
            version: Some(version.into()),
            description: None,
        },
        origin: Some(origin),
        icon: None,
        skill: None,
        mcp_source: None,
        extension: None,
        skills: vec![],
        mcp: vec![],
        settings: None,
        issues: vec![],
    }
}

#[test]
fn limits_planning_actions() {
    let plan = Management { plan: true };
    assert!(plan.read_only());
    assert_eq!(plan.parameters()["type"], "object");
    assert_eq!(plan.parameters()["required"], json!(["action"]));
    assert_eq!(
        plan.parameters()["properties"]["action"]["enum"],
        json!(["list", "search", "inspect", "check_updates"])
    );
    for action in ["list", "search", "inspect", "check_updates"] {
        let schema = plan.parameters();
        assert!(
            schema["oneOf"]
                .as_array()
                .unwrap()
                .iter()
                .any(|variant| variant["properties"]["action"]["const"] == action)
        );
    }
    for arguments in [
        json!({"action":"install","name":"fixture","selection":{"kind":"bundled"}}),
        json!({"action":"update","name":"fixture","expected_revision":7}),
        json!({"action":"uninstall","name":"fixture","expected_revision":7}),
    ] {
        assert_eq!(
            plan.action(arguments).err().unwrap().code,
            ErrorCode::PermissionDenied
        );
    }
    assert_eq!(plan.parameters()["oneOf"].as_array().unwrap().len(), 4);
    assert!(!Management { plan: false }.read_only());
}

#[test]
fn selects_real_catalogs() {
    let Action::Search { source, page, .. } =
        decode(json!({"action":"search","source":"third_party","query":"git"})).unwrap()
    else {
        unreachable!()
    };
    assert_eq!(source, catalog::Source::ThirdParty);
    assert_eq!(page, 1);
    assert!(decode::<Action>(json!({"action":"list","credentials":"private"})).is_err());
    assert!(decode::<Action>(json!({"action":"install","name":"fixture","selection":{"kind":"bundled","credentials":"private"}})).is_err());
    let command = Selection::Git {
        source: skills::Resolved {
            repository: "https://example.invalid/repo.git".into(),
            git_ref: "main".into(),
            commit: "a".repeat(40),
        },
        path: "packages/fixture".into(),
    }
    .command("fixture".into(), 0);
    let Command::InstallPluginSource {
        source,
        path,
        expected_revision,
        ..
    } = command
    else {
        unreachable!()
    };
    assert_eq!(source.commit, "a".repeat(40));
    assert_eq!(path, "packages/fixture");
    assert_eq!(expected_revision, 0);
}

#[test]
fn requires_newer_exact_identity() {
    let worktree = sailry_protocol::WorktreeId::new();
    let current = info("0.1.0", plugin::Origin::Bundled);
    let next = info("0.1.1", plugin::Origin::Bundled);
    assert!(matches!(
        candidate(&next, &current, worktree).unwrap(),
        Command::InstallBundledPlugin {
            expected_revision: 7,
            ..
        }
    ));
    for version in ["0.0.9", "0.1.0", "0.1.0+changed"] {
        assert!(candidate(&info(version, plugin::Origin::Bundled), &current, worktree).is_err());
    }
    let mut changed = next;
    changed.summary.name = "other".into();
    assert!(candidate(&changed, &current, worktree).is_err());
}

#[test]
fn confines_worktree_updates() {
    let worktree = sailry_protocol::WorktreeId::new();
    let current = info("0.1.0", plugin::Origin::Bundled);
    let next = info(
        "0.1.1",
        plugin::Origin::Worktree {
            worktree,
            path: "plugin".into(),
        },
    );
    assert!(
        matches!(candidate(&next,&current,worktree).unwrap(),Command::InstallPlugin { expected_digest:Some(digest),.. } if digest == "frozen")
    );
    assert!(candidate(&next, &current, sailry_protocol::WorktreeId::new()).is_err());
    assert!(candidate(&info("0.1.1", plugin::Origin::Archive), &current, worktree).is_err());
}

#[test]
fn omits_standalone_credentials() {
    let mut value = info("0.1.0", plugin::Origin::Bundled);
    value.mcp_source = Some(plugin::mcp::Definition::StreamableHttp {
        url: "https://private.invalid/mcp".into(),
        headers: vec!["Authorization".into()],
    });
    assert!(feature(&value).is_err());
    assert!(!summary(&value).to_string().contains("private.invalid"));
}
