use super::*;
use sailry_protocol::{Command, RoleId, WorkMode, role};

#[test]
fn lists_safe_exact_catalog_revisions() {
    let profile = role::Profile {
        id: RoleId::new(),
        revision: 9007199254740993,
        key: "review".into(),
        name: "Review".into(),
        appearance: None,
        description: String::new(),
        read_only: true,
        model: None,
        max_turns: None,
        skills: vec![],
        instructions: "Inspect".into(),
    };
    let result=run(bundle("import {listRoles} from 'sailry/sdk'; export async function run() { return listRoles(); }"),"run",json!({}),context(),NodeId([1;32]),CancellationToken::new(),move |request| {
        assert!(matches!(request.command,Command::ListRoles));
        Ok(Output::Roles(vec![profile.clone()]))
    }).unwrap();
    assert_eq!(result[0]["revision"], "9007199254740993");
    assert!(result[0].get("providers").is_none());
    assert!(result[0].get("credentials").is_none());
}

#[test]
fn direct_callbacks_cannot_mutate_catalog() {
    for mode in [WorkMode::Code, WorkMode::Plan] {
        for command in [
            json!({"kind":"remove_role","data":{"role":RoleId::new(),"expected_revision":1}}),
            json!({"kind":"set_session_roles","data":{"session":sailry_protocol::SessionId::new(),"expected_revision":1,"roles":[]}}),
        ] {
            let source = format!(
                "import {{prepareRequest}} from 'sailry/sdk'; export function run() {{ try {{ return prepareRequest({command}); }} catch(error) {{ return error.code; }} }}"
            );
            let state = Arc::new(Mutex::new(execution::State {
                call_id: Some(sailry_protocol::RequestId::new()),
                mode,
                read_only: false,
                turn: None,
                staged_turn: None,
            }));
            assert_eq!(
                run_tool(
                    bundle(&source),
                    "run",
                    json!({}),
                    Environment {
                        context: context(),
                        target: NodeId([1; 32]),
                        stop: CancellationToken::new()
                    },
                    |_| panic!("forbidden role dispatch"),
                    state
                )
                .unwrap(),
                json!("permission_denied")
            );
        }
    }
}
