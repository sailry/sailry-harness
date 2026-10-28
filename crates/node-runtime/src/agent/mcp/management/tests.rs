use super::*;

#[test]
fn validates_action_branches() {
    for plan in [false, true] {
        let schema = Management { plan }.parameters();
        let canonical = jsonschema::draft202012::options().build(&schema).unwrap();
        for (arguments, valid) in [
            (json!({"action":"list"}), true),
            (
                json!({"action":"uninstall","name":"mcp-fixture","expected_revision":1}),
                true,
            ),
            (json!({"action":"uninstall","name":"mcp-fixture"}), false),
            (json!({"action":"list","name":"mcp-fixture"}), false),
            (json!({"action":"install"}), false),
            (json!({"action":"unknown"}), false),
        ] {
            assert_eq!(canonical.is_valid(&arguments), valid, "{arguments}");
        }
        assert!(schema.get("oneOf").is_some());
    }
}

fn definition() -> Server {
    Server::StreamableHttp {
        url: "https://example.invalid/mcp".into(),
        headers: vec!["Authorization".into()],
    }
}

#[test]
fn accepts_slots_without_private_values() {
    let args: Install = decode(json!({"name":"mcp-fixture","definition":definition()})).unwrap();
    let Command::InstallMcp {
        secrets,
        expected_revision,
        ..
    } = install(args)
    else {
        unreachable!()
    };
    assert_eq!(expected_revision, 0);
    assert_eq!(secrets.len(), 1);
    assert!(matches!(secrets["Authorization"], SecretUpdate::Clear));
    for arguments in [
        json!({"name":"mcp-fixture","definition":definition(),"secrets":{"Authorization":"private"}}),
        json!({"name":"mcp-fixture","definition":{"type":"streamable-http","url":"https://example.invalid/mcp","headers":{"Authorization":"private"}}}),
    ] {
        assert!(decode::<Install>(arguments).is_err());
    }
    assert!(decode::<Update>(json!({"name":"mcp-fixture","expected_revision":1,"secrets":{"Authorization":{"kind":"replace","value":"private"}}})).is_err());
}

#[test]
fn retains_credentials_unless_explicitly_cleared() {
    let previous = definition();
    for clear in [false, true] {
        let args: Update = decode(json!({"name":"mcp-fixture","expected_revision":7,"clear_credentials":if clear {vec!["Authorization"]} else {Vec::new()}})).unwrap();
        let Command::InstallMcp {
            definition,
            secrets,
            expected_revision,
            ..
        } = update(args, &previous).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(definition, previous);
        assert_eq!(expected_revision, 7);
        assert_eq!(secrets.len(), 1);
        assert!(if clear {
            matches!(secrets["Authorization"], SecretUpdate::Clear)
        } else {
            matches!(secrets["Authorization"], SecretUpdate::Keep)
        });
    }
    for arguments in [
        json!({"name":"mcp-fixture","expected_revision":7,"clear_credentials":["OTHER"]}),
        json!({"name":"mcp-fixture","expected_revision":7,"clear_credentials":["Authorization","Authorization"]}),
        json!({"name":"mcp-fixture","expected_revision":7,"definition":{"type":"streamable-http","url":"https://example.invalid/mcp","headers":["Other"]}}),
        json!({"name":"mcp-fixture","expected_revision":7,"definition":{"type":"sse","url":"https://example.invalid/mcp","headers":["Authorization"]}}),
    ] {
        assert!(update(decode(arguments).unwrap(), &previous).is_err());
    }
}

#[test]
fn limits_mutations_to_standalone_revisions() {
    let directory = tempfile::tempdir().unwrap();
    let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
    let mut info = host.install_mcp("mcp-fixture", &definition()).unwrap();
    info.summary.revision = 7;
    assert!(standalone(&info, 7).is_ok());
    for revision in [0, 6, 8] {
        assert_eq!(
            standalone(&info, revision).unwrap_err().code,
            ErrorCode::RevisionConflict
        );
    }
    let public = serde_json::to_string(&summary(&info)).unwrap();
    assert!(!public.contains("example.invalid"));
    assert!(public.contains("Authorization"));
    info.mcp_source = None;
    assert_eq!(
        standalone(&info, 7).unwrap_err().code,
        ErrorCode::PermissionDenied
    );
}

#[test]
fn exposes_only_inventory_as_read_only() {
    assert!(Operation::List.read_only());
    for operation in [Operation::Install, Operation::Update, Operation::Uninstall] {
        assert!(!operation.read_only());
        assert_eq!(operation.parameters()["additionalProperties"], false);
        assert!(
            operation.parameters()["properties"]
                .get("secrets")
                .is_none()
        );
    }
}

#[test]
fn scopes_actions_to_the_frozen_mode() {
    let list = json!({"action":"list"});
    let writes = [
        json!({"action":"install","name":"mcp-fixture","definition":definition()}),
        json!({"action":"update","name":"mcp-fixture","expected_revision":7}),
        json!({"action":"uninstall","name":"mcp-fixture","expected_revision":7}),
    ];
    for plan in [false, true] {
        let management = Management { plan };
        let schema = management.parameters();
        assert_eq!(schema, Management { plan: false }.parameters());
        let validator = jsonschema::draft202012::options().build(&schema).unwrap();
        assert_eq!(management.name(), "manage_mcp");
        assert_eq!(management.read_only(), plan);
        assert!(validator.is_valid(&list));
        assert!(management.arguments(list.clone()).is_ok());
        for args in &writes {
            assert!(validator.is_valid(args));
            let result = management.arguments(args.clone());
            if plan {
                assert_eq!(result.err().unwrap().code, ErrorCode::PermissionDenied);
            } else {
                assert!(result.is_ok());
            }
        }
        assert!(!validator.is_valid(&json!({"action":"list","secrets":{"key":"private"}})));
        assert!(management.arguments(json!({"action":"unknown"})).is_err());
    }
}
