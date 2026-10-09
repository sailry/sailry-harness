use super::*;
use crate::store::agent::delegation::fixture::{invoke, profile, setup, start};
use serde_json::json;

#[test]
fn defaults_to_parent_config() {
    for roster in [false, true] {
        for explicit in [false, true] {
            let mut fixture = setup();
            if roster {
                profile(&mut fixture);
            }
            let parent = start(&mut fixture);
            let config =
                super::super::super::permissions::config(&fixture.database.connection, parent)
                    .unwrap();
            let mut args = json!({"task": "第一轮：读取并总结"});
            if explicit {
                args["role"] = Value::Null;
            }
            let child = invoke(&mut fixture, parent, args).unwrap();
            assert_eq!(child.turn.config, config);
            assert_eq!(child.message.text, "第一轮：读取并总结");
            assert!(child.child.unwrap().role.is_none());
            assert!(
                read(&fixture.database.connection, child.turn.session)
                    .unwrap()
                    .unwrap()
                    .role
                    .is_none()
            );
            assert!(child.turn.roles.profiles.is_empty());
        }
    }
}

#[test]
fn selects_only_admitted_roles() {
    let mut fixture = setup();
    let role = profile(&mut fixture);
    let parent = start(&mut fixture);
    assert_eq!(
        invoke(
            &mut fixture,
            parent,
            json!({"role": "unknown", "task": "Task"}),
        )
        .err()
        .unwrap()
        .code,
        ErrorCode::InvalidRequest
    );
    let child = invoke(
        &mut fixture,
        parent,
        json!({"role": "review", "task": "Task"}),
    )
    .unwrap();
    assert_eq!(child.child.unwrap().role.unwrap(), role);
}

#[test]
fn freezes_read_only_execution() {
    let mut fixture = setup();
    let parent = start(&mut fixture);
    let original =
        super::super::super::permissions::config(&fixture.database.connection, parent).unwrap();
    let shipped = crate::store::roles::list(&fixture.database.connection)
        .unwrap()
        .into_iter()
        .find(|role| role.key == "explore")
        .unwrap();
    crate::store::roles::remove(&fixture.database.connection, shipped.id, shipped.revision)
        .unwrap();
    let child = invoke(
        &mut fixture,
        parent,
        json!({"role":"explore","task":"Inspect sources"}),
    )
    .unwrap();
    let mut expected = original;
    expected.mode = WorkMode::Plan;
    assert_eq!(child.turn.config, expected);
    assert_eq!(child.child.as_ref().unwrap().role, Some(shipped));
    assert_eq!(
        super::super::super::permissions::source(
            &fixture.database.connection,
            child.turn.id,
            &ToolConfirmationRequest {
                tool_name: crate::plugins::tools::alias("files", "write_file"),
                function_call_id: Some("write-attempt".into()),
                args: json!({"path":"changed.txt","text":"Changed","expected_revision":null}),
            },
        )
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
}

#[test]
fn uses_constraints_independently_of_names() {
    for (key, read_only, mode) in [
        ("explore", false, WorkMode::Code),
        ("scan", true, WorkMode::Plan),
    ] {
        let mut fixture = setup();
        let mut role = crate::store::roles::list(&fixture.database.connection)
            .unwrap()
            .remove(0);
        role.key = key.into();
        role.read_only = read_only;
        let role =
            crate::store::roles::put(&fixture.database.connection, &role, role.revision).unwrap();
        crate::store::agent::tests::command(
            &mut fixture.database,
            &fixture.events,
            Command::SetSessionRoles {
                session: fixture.session,
                expected_revision: 1,
                roles: vec![role.reference()],
            },
        );
        let parent = start(&mut fixture);
        let child = invoke(
            &mut fixture,
            parent,
            json!({"role":key,"task":"Inspect sources"}),
        )
        .unwrap();
        assert_eq!(child.turn.config.mode, mode);
        assert_eq!(child.child.unwrap().role, Some(role));
    }
}
