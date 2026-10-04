use super::*;

mod computer;

mod storage {
    use super::*;
    fn fixture(operation: sailry_protocol::tool::Operation, mode: WorkMode) -> Fixture {
        let fixture = Fixture::with_mode(Permission::Ask, mode);
        let db = &fixture.database.connection;
        let mut package = crate::store::plugins::get(db, "files").unwrap().unwrap();
        package.summary.name = "private-state".into();
        package.summary.digest = blake3::hash(b"private-state-test").to_hex().to_string();
        package.summary.settings_revision = 0;
        package.settings = None;
        let extension = package.extension.as_mut().unwrap();
        extension.actions = vec![plugin::Action::ReadStorage, plugin::Action::WriteStorage];
        extension.settings_schema = None;
        extension.tools = vec![
            serde_json::from_value(
                json!({"name":"save","operation":operation,"description":"Save private values"}),
            )
            .unwrap(),
        ];
        db.execute(
            "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)",
            params![
                package.summary.digest,
                package.summary.name,
                encode(&package).unwrap()
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO plugins(name,revision,installed,body) VALUES(?1,1,1,?2)",
            params![package.summary.name, encode(&package).unwrap()],
        )
        .unwrap();
        let mut references = crate::store::agent::plugins::read(db, fixture.turn).unwrap();
        references.push(package.summary.reference());
        db.execute(
            "UPDATE turns SET plugins=?1 WHERE id=?2",
            params![encode(&references).unwrap(), fixture.turn.to_string()],
        )
        .unwrap();
        fixture
    }

    #[test]
    fn uses_captured_authority_and_denies_planning() {
        for operation in [
            sailry_protocol::tool::Operation::SetValue,
            sailry_protocol::tool::Operation::DeleteValue,
            sailry_protocol::tool::Operation::StorageTransaction,
        ] {
            for mode in [WorkMode::Code, WorkMode::Plan] {
                let fixture = fixture(operation, mode);
                let mut request = fixture.request.clone();
                request.tool_name = crate::plugins::tools::alias("private-state", "save");
                let result = crate::store::agent::permissions::source(
                    &fixture.database.connection,
                    fixture.turn,
                    &request,
                );
                if mode == WorkMode::Plan {
                    assert_eq!(result.unwrap_err().code, ErrorCode::PermissionDenied);
                } else {
                    assert_eq!(result.unwrap(), ApprovalSource::Storage);
                    request.tool_name = crate::plugins::tools::alias("other-state", "save");
                    assert_eq!(
                        crate::store::agent::permissions::source(
                            &fixture.database.connection,
                            fixture.turn,
                            &request
                        )
                        .unwrap(),
                        ApprovalSource::User
                    );
                }
            }
        }
    }

    #[test]
    fn preserves_reads_and_revokes_writes() {
        use sailry_protocol::plugin::{storage::Index, transaction::Operation};
        let mut fixture = fixture(
            sailry_protocol::tool::Operation::StorageTransaction,
            WorkMode::Code,
        );
        let package = crate::store::plugins::get(&fixture.database.connection, "private-state")
            .unwrap()
            .unwrap();
        let session =
            crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
        let context = plugin::Context {
            invocation: None,
            turn: Some(fixture.turn),
            surface: Default::default(),
            package: package.summary.reference(),
            worktree: Some(session.worktree),
            session: Some(fixture.session),
        };
        let index = Index {
            fields: ["title".into(), "body".into()],
            tags: vec![],
            order: 1,
        };
        let reads = [
            Command::ReadPluginValue { key: "item".into() },
            Command::ListPluginKeys {
                prefix: String::new(),
                after: None,
                limit: 1,
            },
            Command::SearchPluginValues(serde_json::from_value(json!({"terms":[]})).unwrap()),
        ];
        let writes = [
            Command::WritePluginValue {
                key: "item".into(),
                value: json!(true),
                expected_revision: 0,
            },
            Command::WriteIndexedPluginValue {
                key: "item".into(),
                value: json!(true),
                index: index.clone(),
                expected_revision: 0,
            },
            Command::RemovePluginValue {
                key: "item".into(),
                expected_revision: 0,
            },
            Command::PluginTransaction {
                operations: vec![Operation::Index {
                    key: "item".into(),
                    value: json!(true),
                    index,
                    expected_revision: 0,
                }],
            },
        ];
        for command in reads.iter().chain(&writes) {
            let request =
                Request::new(fixture.database.node, command.clone()).with_plugin(context.clone());
            fixture
                .database
                .check_plugin(fixture.database.node, &request)
                .unwrap();
        }
        command(
            &mut fixture.database,
            &fixture.events,
            Command::SetPluginEnabled {
                name: "private-state".into(),
                expected_revision: package.summary.revision,
                enabled: false,
            },
        );
        for command in reads {
            let mut request =
                Request::new(fixture.database.node, command).with_plugin(context.clone());
            fixture
                .database
                .check_plugin(fixture.database.node, &request)
                .unwrap();
            request.plugin.as_mut().unwrap().surface = plugin::desktop::Surface::Settings;
            assert_eq!(
                fixture
                    .database
                    .check_plugin(fixture.database.node, &request)
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        for command in writes {
            let mut request =
                Request::new(fixture.database.node, command).with_plugin(context.clone());
            assert_eq!(
                fixture
                    .database
                    .check_plugin(fixture.database.node, &request)
                    .unwrap_err()
                    .code,
                ErrorCode::NotConfigured
            );
            request.plugin.as_mut().unwrap().surface = plugin::desktop::Surface::Settings;
            assert_eq!(
                fixture
                    .database
                    .check_plugin(fixture.database.node, &request)
                    .unwrap_err()
                    .code,
                ErrorCode::NotConfigured
            );
        }
    }
}

#[test]
fn consumes_automatic_authority_once() {
    for (mode, source) in [
        (Permission::Project, ApprovalSource::Project),
        (Permission::Full, ApprovalSource::Full),
    ] {
        let mut fixture = Fixture::with_permission(mode);
        let approval = fixture.begin();
        assert_eq!(approval.state, ApprovalState::Approved);
        assert_eq!(approval.source, source);
        let mut changed = fixture.request.clone();
        changed.args["text"] = json!("unapproved content");
        assert!(authorize(&mut fixture.database.connection, fixture.turn, &changed).is_err());
        authorize(
            &mut fixture.database.connection,
            fixture.turn,
            &fixture.request,
        )
        .unwrap();
        assert!(
            authorize(
                &mut fixture.database.connection,
                fixture.turn,
                &fixture.request
            )
            .is_err()
        );
    }
}

#[test]
fn uses_admitted_revision() {
    for (admitted, current, expected) in [
        (Permission::Ask, Permission::Full, ApprovalState::Pending),
        (
            Permission::Project,
            Permission::Ask,
            ApprovalState::Approved,
        ),
        (Permission::Full, Permission::Ask, ApprovalState::Approved),
    ] {
        let mut fixture = Fixture::with_permission(admitted);
        let body: Vec<u8> = fixture
            .database
            .connection
            .query_row(
                "SELECT config FROM session_revisions WHERE session=?1 AND revision=1",
                [fixture.session.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let mut config: SessionConfig = serde_json::from_slice(&body).unwrap();
        config.permission = current;
        command(
            &mut fixture.database,
            &fixture.events,
            Command::SetSessionConfig {
                session: fixture.session,
                expected_revision: 1,
                config,
            },
        );
        assert_eq!(fixture.begin().state, expected);
    }
}

#[test]
fn requires_command_authority() {
    for mode in [Permission::Ask, Permission::Project, Permission::Full] {
        let fixture = Fixture::with_permission(mode);
        for tool_name in [
            "plugin_fixture_command",
            "create_worktree",
            "register_worktree",
            "remove_worktree",
            "unknown_tool",
        ] {
            let request = ToolConfirmationRequest {
                tool_name: tool_name.into(),
                function_call_id: Some("command".into()),
                args: json!({"command": "printf content"}),
            };
            let source = crate::store::agent::permissions::source(
                &fixture.database.connection,
                fixture.turn,
                &request,
            )
            .unwrap();
            let expected = match (mode, tool_name) {
                (Permission::Full, name) if name != "unknown_tool" => ApprovalSource::Full,
                _ => ApprovalSource::User,
            };
            assert_eq!(source, expected);
        }
    }
}

#[test]
fn rejects_planned_worktree_changes() {
    for permission in [Permission::Ask, Permission::Project, Permission::Full] {
        let fixture = Fixture::with_mode(permission, WorkMode::Plan);
        for tool in ["create_worktree", "register_worktree", "remove_worktree"] {
            let mut request = fixture.request.clone();
            request.tool_name = tool.into();
            assert_eq!(
                crate::store::agent::permissions::source(
                    &fixture.database.connection,
                    fixture.turn,
                    &request,
                )
                .unwrap_err()
                .code,
                ErrorCode::PermissionDenied,
            );
        }
    }
}

#[test]
fn scopes_office_writes_to_project_authority() {
    for mode in [WorkMode::Code, WorkMode::Plan] {
        for permission in [Permission::Ask, Permission::Project, Permission::Full] {
            let fixture = Fixture::with_mode(permission, mode);
            {
                let tool = crate::plugins::tools::alias("files", "export_pdf");
                let mut request = fixture.request.clone();
                request.tool_name = tool;
                let source = crate::store::agent::permissions::source(
                    &fixture.database.connection,
                    fixture.turn,
                    &request,
                );
                if mode == WorkMode::Plan {
                    assert_eq!(source.unwrap_err().code, ErrorCode::PermissionDenied);
                } else {
                    assert_eq!(
                        source.unwrap(),
                        match permission {
                            Permission::Ask => ApprovalSource::User,
                            Permission::Project => ApprovalSource::Project,
                            Permission::Full => ApprovalSource::Full,
                        }
                    );
                }
            }
        }
    }
}
