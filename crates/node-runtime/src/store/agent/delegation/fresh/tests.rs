use super::*;
use crate::store::agent::delegation::fixture::{invoke, profile, setup, start};
use serde_json::json;

#[tokio::test]
async fn defaults_to_parent_config() {
    for roster in [false, true] {
        for explicit in [false, true] {
            let mut fixture = setup().await;
            if roster {
                profile(&mut fixture).await;
            }
            let parent = start(&mut fixture).await;
            let config =
                super::super::super::permissions::config(&fixture.database.connection, parent)
                    .await
                    .unwrap();
            let mut args = json!({"task": "第一轮：读取并总结"});
            if explicit {
                args["role"] = Value::Null;
            }
            let child = invoke(&mut fixture, parent, args).await.unwrap();
            assert_eq!(child.turn.config, config);
            assert_eq!(child.message.text, "第一轮：读取并总结");
            assert!(child.child.unwrap().role.is_none());
            assert!(
                read(&fixture.database.connection, child.turn.session)
                    .await
                    .unwrap()
                    .unwrap()
                    .role
                    .is_none()
            );
            assert!(child.turn.roles.profiles.is_empty());
        }
    }
}

#[tokio::test]
async fn selects_only_admitted_roles() {
    let mut fixture = setup().await;
    let role = profile(&mut fixture).await;
    let parent = start(&mut fixture).await;
    assert_eq!(
        invoke(
            &mut fixture,
            parent,
            json!({"role": "unknown", "task": "Task"}),
        )
        .await
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
    .await
    .unwrap();
    assert_eq!(child.child.unwrap().role.unwrap(), role);
}

#[tokio::test]
async fn freezes_read_only_execution() {
    let mut fixture = setup().await;
    let parent = start(&mut fixture).await;
    let original = super::super::super::permissions::config(&fixture.database.connection, parent)
        .await
        .unwrap();
    let shipped = crate::store::roles::list(&fixture.database.connection)
        .await
        .unwrap()
        .into_iter()
        .find(|role| role.key == "explore")
        .unwrap();
    crate::store::roles::remove(&fixture.database.connection, shipped.id, shipped.revision)
        .await
        .unwrap();
    let child = invoke(
        &mut fixture,
        parent,
        json!({"role":"explore","task":"Inspect sources"}),
    )
    .await
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
        .await
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
}

#[tokio::test]
async fn inherits_captured_mode_after_removal() {
    let mut fixture = setup().await;
    let info = crate::plugins::modes::tests::package("review-mode", "review", false);
    let db = &fixture.database.connection;
    db.execute(
        "INSERT INTO plugins(name,revision,installed,body) VALUES(?1,1,1,?2)",
        params![info.summary.name.as_str(), encode(&info).unwrap()],
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)",
        params![
            info.summary.digest.as_str(),
            info.summary.name.as_str(),
            encode(&info).unwrap()
        ],
    )
    .await
    .unwrap();
    let session = crate::store::commands::session(db, fixture.session)
        .await
        .unwrap();
    let mut config = session.config;
    config.mode = WorkMode::Plan;
    config.permission = Permission::Full;
    config.options = serde_json::from_value(json!({"review-mode":{"mode":"review"}})).unwrap();
    crate::store::agent::tests::command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionConfig {
            session: fixture.session,
            expected_revision: session.revision,
            config: config.clone(),
        },
    )
    .await;
    let parent = start(&mut fixture).await;
    fixture
        .database
        .connection
        .execute(
            "UPDATE plugins SET installed=0 WHERE name='review-mode'",
            (),
        )
        .await
        .unwrap();
    let child = invoke(&mut fixture, parent, json!({"task":"Inspect changes"}))
        .await
        .unwrap();
    assert_eq!(child.turn.config, config);
    assert_eq!(
        crate::plugins::modes::selected(&child.plugins, &child.turn.config)
            .unwrap()
            .unwrap()
            .0
            .summary
            .reference(),
        info.summary.reference()
    );
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
            }
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
}

#[tokio::test]
async fn uses_constraints_independently_of_names() {
    for (key, read_only, mode) in [
        ("explore", false, WorkMode::Code),
        ("scan", true, WorkMode::Plan),
    ] {
        let mut fixture = setup().await;
        let mut role = crate::store::roles::list(&fixture.database.connection)
            .await
            .unwrap()
            .remove(0);
        role.key = key.into();
        role.read_only = read_only;
        let role = crate::store::roles::put(&fixture.database.connection, &role, role.revision)
            .await
            .unwrap();
        crate::store::agent::tests::command(
            &mut fixture.database,
            &fixture.events,
            Command::SetSessionRoles {
                session: fixture.session,
                expected_revision: 1,
                roles: vec![role.reference()],
            },
        )
        .await;
        let parent = start(&mut fixture).await;
        let child = invoke(
            &mut fixture,
            parent,
            json!({"role":key,"task":"Inspect sources"}),
        )
        .await
        .unwrap();
        assert_eq!(child.turn.config.mode, mode);
        assert_eq!(child.child.unwrap().role, Some(role));
    }
}

#[tokio::test]
async fn freezes_role_channels_at_parent_admission() {
    use crate::store::agent::tests::command;
    use crate::store::agent::tests::fixture::Fixture;
    use sailry_protocol::conversation::ModelApi;
    let mut fixture = setup().await;
    let provider_id = ProviderId::new();
    let configure = async |fixture: &mut Fixture, value: u32| {
        let digest = format!("{value:064x}");
        let info:plugin::Info=serde_json::from_value(json!({"summary":{"name":"channel-example","revision":value,"digest":digest,"settings_revision":0,"enabled":true},"extension":{"api_version":"v1","model_providers":[{"id":"example","name":"Example","routes":[{"protocol":"openai-chat-completions","endpoint":"http://localhost:11434/v1"}],"policies":[{"models":["fixture"],"body":{"example":value}}]}]}})).unwrap();
        let db = &fixture.database.connection;
        db.execute("INSERT OR REPLACE INTO plugins(name,revision,settings_revision,installed,body) VALUES('channel-example',?1,0,1,?2)",params![value,encode(&info).unwrap()]).await.unwrap();
        db.execute("INSERT OR REPLACE INTO plugin_packages(digest,name,body) VALUES(?1,'channel-example',?2)",params![digest,encode(&info).unwrap()]).await.unwrap();
        info.summary.reference()
    };
    let original = configure(&mut fixture, 1).await;
    let provider:Provider=serde_json::from_value(json!({"id":provider_id,"revision":0,"name":"Example","api":"chat_completions","authentication":"api_key","endpoint":"http://localhost:11434/v1","enabled":true,"models":[{"id":"fixture","context":4096,"output":512}],"default_model":"fixture","credential":null,"preset":{"source":{"kind":"package","data":{"name":"channel-example"}},"id":"example","route":null}})).unwrap();
    let Output::Provider(_) = command(
        &mut fixture.database,
        &fixture.events,
        Command::PutProvider {
            provider,
            expected_revision: 0,
        },
    )
    .await
    else {
        panic!("provider expected")
    };
    let mut role = profile(&mut fixture).await;
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
    )
    .await
    else {
        panic!("role expected")
    };
    let session = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionRoles {
            session: session.id,
            expected_revision: session.revision,
            roles: vec![role.reference()],
        },
    )
    .await;
    let first = start(&mut fixture).await;
    let updated = configure(&mut fixture, 2).await;
    let first_roster = crate::store::sessions::roles::for_turn(&fixture.database.connection, first)
        .await
        .unwrap();
    assert_eq!(
        first_roster.providers[0].binding.as_ref().unwrap().package,
        Some(original.clone())
    );
    let child = invoke(
        &mut fixture,
        first,
        json!({"role":"review","task":"Inspect the frozen channel"}),
    )
    .await
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
    super::super::fixture::finish(&mut fixture, child.turn.id).await;
    super::super::fixture::finish(&mut fixture, first).await;
    let second = start(&mut fixture).await;
    let roster = crate::store::sessions::roles::for_turn(&fixture.database.connection, second)
        .await
        .unwrap();
    assert_eq!(
        roster.providers[0].binding.as_ref().unwrap().package,
        Some(updated)
    );
    assert_eq!(
        crate::store::sessions::roles::for_turn(&fixture.database.connection, first)
            .await
            .unwrap(),
        first_roster
    );
}

#[tokio::test]
async fn preserves_roles_across_reopen() {
    use crate::store::{
        agent::tests::{command, fixture::Fixture},
        database::Database,
    };
    let mut fixture = setup().await;
    let mut role = profile(&mut fixture).await;
    let turn = fixture.queued().await;
    let original = crate::store::sessions::roles::for_turn(&fixture.database.connection, turn)
        .await
        .unwrap();
    role.instructions = "Newly selected instructions".into();
    let Output::Role(role) = command(
        &mut fixture.database,
        &fixture.events,
        Command::PutRole {
            expected_revision: role.revision,
            role,
        },
    )
    .await
    else {
        panic!("role expected")
    };
    let current = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionRoles {
            session: current.id,
            expected_revision: current.revision,
            roles: vec![role.reference()],
        },
    )
    .await;
    let current = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_ne!(current.roles, original);
    let node = fixture.database.node;
    let Fixture {
        database, _root, ..
    } = fixture;
    database.close().await.unwrap();
    let database = Database::open(
        &_root.path().canonicalize().unwrap().join("node.turso"),
        node,
        None,
    )
    .await
    .unwrap();
    let body: Vec<u8> = database
        .connection
        .prepare("SELECT roles FROM turns WHERE id=?1")
        .await
        .unwrap()
        .query_row([turn.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<role::Snapshot>(&body).unwrap(),
        original
    );
    assert_eq!(
        crate::store::sessions::roles::for_turn(&database.connection, turn)
            .await
            .unwrap(),
        original
    );
    assert_eq!(
        crate::store::commands::session(&database.connection, current.id)
            .await
            .unwrap()
            .roles,
        current.roles
    );
}
