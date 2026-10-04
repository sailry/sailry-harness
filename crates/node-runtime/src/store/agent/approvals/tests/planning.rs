use super::*;
use sailry_protocol::WorkMode;

#[test]
fn rejects_mutating_approvals() {
    for permission in [Permission::Ask, Permission::Project, Permission::Full] {
        let mut fixture = Fixture::with_mode(permission, WorkMode::Plan);
        for tool in [
            "write_file",
            "plugin_fixture_command",
            "mcp_fixture",
            "unknown_tool",
        ] {
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
        assert_eq!(
            begin(
                &mut fixture.database,
                fixture.turn,
                &fixture.request,
                &fixture.events
            )
            .unwrap_err()
            .code,
            ErrorCode::PermissionDenied,
        );
        assert!(
            list(&fixture.database.connection, fixture.session, &[])
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn preserves_admitted_authority() {
    for (admitted, current) in [
        (WorkMode::Plan, WorkMode::Code),
        (WorkMode::Code, WorkMode::Plan),
    ] {
        let mut fixture = Fixture::with_mode(Permission::Full, admitted);
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
        config.mode = current;
        command(
            &mut fixture.database,
            &fixture.events,
            Command::SetSessionConfig {
                session: fixture.session,
                expected_revision: 1,
                config,
            },
        );
        let result = begin(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            &fixture.events,
        );
        if admitted == WorkMode::Plan {
            assert_eq!(result.unwrap_err().code, ErrorCode::PermissionDenied);
        } else {
            assert_eq!(result.unwrap().state, ApprovalState::Approved);
        }
    }
}

#[test]
fn rejects_incompatible_modes() {
    for owner in ["defaults", "revision", "event", "receipt"] {
        let fixture = Fixture::with_mode(Permission::Full, WorkMode::Code);
        let db = &fixture.database.connection;
        let body: Vec<u8> = db
            .query_row(
                "SELECT config FROM session_revisions WHERE session=?1 AND revision=1",
                [fixture.session.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let mut config: serde_json::Value = serde_json::from_slice(&body).unwrap();
        config.as_object_mut().unwrap().remove("mode");
        let defaults = json!({"revision": 1, "config": config});
        match owner {
            "defaults" => {
                db.execute(
                    "INSERT INTO defaults(singleton,revision,config) VALUES(1,1,?1)",
                    [serde_json::to_vec(&config).unwrap()],
                )
                .unwrap();
            }
            "revision" => {
                db.execute(
                    "UPDATE session_revisions SET config=?1 WHERE session=?2",
                    params![
                        serde_json::to_vec(&config).unwrap(),
                        fixture.session.to_string()
                    ],
                )
                .unwrap();
            }
            "event" => {
                db.execute(
                    "INSERT INTO events(body) VALUES(?1)",
                    [
                        serde_json::to_vec(&json!({"kind": "defaults_changed", "data": defaults}))
                            .unwrap(),
                    ],
                )
                .unwrap();
            }
            "receipt" => {
                db.execute(
                "INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed',?4)",
                params![&fixture.database.node.0[..], RequestId::new().to_string(), b"fixture".as_slice(),
                    json!({"Ok": {"kind": "defaults", "data": defaults}}).to_string()],
            ).unwrap();
            }
            _ => unreachable!(),
        }
        let node = fixture.database.node;
        fixture.database.close().unwrap();
        let before = std::fs::read(&fixture.path).unwrap();
        assert!(Database::open(&fixture.path, node, None).is_err());
        assert_eq!(std::fs::read(&fixture.path).unwrap(), before);
    }
}
