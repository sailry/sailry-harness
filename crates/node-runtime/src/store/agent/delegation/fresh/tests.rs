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
