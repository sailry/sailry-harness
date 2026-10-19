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
                acting: None,
                origin: None,
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

#[test]
fn freezes_role_channels_at_parent_admission() {
    use crate::store::agent::tests::command;
    use crate::store::agent::tests::fixture::Fixture;
    use sailry_protocol::conversation::ModelApi;
    let mut fixture = setup();
    let provider_id = ProviderId::new();
    let configure = |fixture: &mut Fixture, value: u32| {
        let digest = format!("{value:064x}");
        let info:plugin::Info=serde_json::from_value(json!({"summary":{"name":"channel-example","revision":value,"digest":digest,"settings_revision":0,"enabled":true},"extension":{"api_version":"v1","model_providers":[{"id":"example","name":"Example","routes":[{"protocol":"openai-chat-completions","endpoint":"http://localhost:11434/v1"}],"policies":[{"models":["fixture"],"body":{"example":value}}]}]}})).unwrap();
        let db = &fixture.database.connection;
        db.execute("INSERT OR REPLACE INTO plugins(name,revision,settings_revision,installed,body) VALUES('channel-example',?1,0,1,?2)",params![value,encode(&info).unwrap()]).unwrap();
        db.execute("INSERT OR REPLACE INTO plugin_packages(digest,name,body) VALUES(?1,'channel-example',?2)",params![digest,encode(&info).unwrap()]).unwrap();
        info.summary.reference()
    };
    let original = configure(&mut fixture, 1);
    let provider:Provider=serde_json::from_value(json!({"id":provider_id,"revision":0,"name":"Example","api":"chat_completions","authentication":"api_key","endpoint":"http://localhost:11434/v1","enabled":true,"models":[{"id":"fixture","context":4096,"output":512}],"default_model":"fixture","credential":null,"preset":{"source":{"kind":"package","data":{"name":"channel-example"}},"id":"example","route":null}})).unwrap();
    let Output::Provider(_) = command(
        &mut fixture.database,
        &fixture.events,
        Command::PutProvider {
            provider,
            expected_revision: 0,
        },
    ) else {
        panic!("provider expected")
    };
    let mut role = profile(&mut fixture);
    role.model = Some(role::Model {
        provider: provider_id,
        model: "fixture".into(),
        effort: Some(Effort::Default),
    });
    let Output::Role(role) = command(
        &mut fixture.database,
        &fixture.events,
        Command::PutRole {
            role: role.clone(),
            expected_revision: role.revision,
        },
    ) else {
        panic!("role expected")
    };
    let session =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionRoles {
            session: session.id,
            expected_revision: session.revision,
            roles: vec![role.reference()],
        },
    );
    let first = start(&mut fixture);
    let updated = configure(&mut fixture, 2);
    let first_roster =
        crate::store::sessions::roles::for_turn(&fixture.database.connection, first).unwrap();
    assert_eq!(
        first_roster.providers[0].binding.as_ref().unwrap().package,
        Some(original.clone())
    );
    let child = invoke(
        &mut fixture,
        first,
        json!({"role":"review","task":"Inspect the frozen channel"}),
    )
    .unwrap();
    assert_eq!(
        child.provider.as_ref().unwrap().api,
        ModelApi::ChatCompletions
    );
    assert_eq!(
        child
            .provider
            .as_ref()
            .unwrap()
            .binding
            .as_ref()
            .unwrap()
            .package,
        Some(original)
    );
    super::super::fixture::finish(&mut fixture, child.turn.id);
    super::super::fixture::finish(&mut fixture, first);
    let second = start(&mut fixture);
    let roster =
        crate::store::sessions::roles::for_turn(&fixture.database.connection, second).unwrap();
    assert_eq!(
        roster.providers[0].binding.as_ref().unwrap().package,
        Some(updated)
    );
    assert_eq!(
        crate::store::sessions::roles::for_turn(&fixture.database.connection, first).unwrap(),
        first_roster
    );
}

#[test]
fn upgrades_from_the_original_role_revision() {
    use crate::store::{
        agent::tests::{command, fixture::Fixture},
        database::Database,
    };
    let mut fixture = setup();
    let mut role = profile(&mut fixture);
    let turn = fixture.queued();
    let original =
        crate::store::sessions::roles::for_turn(&fixture.database.connection, turn).unwrap();
    role.instructions = "Newly selected instructions".into();
    let Output::Role(role) = command(
        &mut fixture.database,
        &fixture.events,
        Command::PutRole {
            expected_revision: role.revision,
            role,
        },
    ) else {
        panic!("role expected")
    };
    let current =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionRoles {
            session: current.id,
            expected_revision: current.revision,
            roles: vec![role.reference()],
        },
    );
    let current =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
    assert_ne!(current.roles, original);
    fixture
        .database
        .connection
        .execute_batch("ALTER TABLE turns DROP COLUMN roles")
        .unwrap();
    let node = fixture.database.node;
    let Fixture {
        database, _root, ..
    } = fixture;
    database.close().unwrap();
    let database = Database::open(
        &_root.path().canonicalize().unwrap().join("node.sqlite3"),
        node,
        None,
    )
    .unwrap();
    let body: Vec<u8> = database
        .connection
        .query_row(
            "SELECT roles FROM turns WHERE id=?1",
            [turn.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<role::Snapshot>(&body).unwrap(),
        original
    );
    assert_eq!(
        crate::store::sessions::roles::for_turn(&database.connection, turn).unwrap(),
        original
    );
    assert_eq!(
        crate::store::commands::session(&database.connection, current.id)
            .unwrap()
            .roles,
        current.roles
    );
}
