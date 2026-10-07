use super::super::fixture::{Fixture, finish, invoke, profile, setup, start};
use super::*;
use crate::store::agent::tests::command;
use serde_json::json;

fn install_provider(fixture: &mut Fixture) -> Provider {
    let session =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
    let provider = Provider {
        id: session.config.provider,
        revision: 0,
        name: "Captured fixture".into(),
        api: ModelApi::Responses,
        authentication: Authentication::ApiKey,
        endpoint: "https://captured.invalid/v1".into(),
        enabled: true,
        models: vec![],
        default_model: "fixture".into(),
        credential: None,
        options: None,
        oauth: None,
    };
    let Output::Provider(provider) = command(
        &mut fixture.database,
        &fixture.events,
        Command::PutProvider {
            provider,
            expected_revision: 0,
        },
    ) else {
        panic!("provider expected")
    };
    provider
}

fn settings_package(fixture: &Fixture) -> plugin::Info {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/examples")
        .canonicalize()
        .unwrap();
    let info = crate::plugins::Host::new(Some(fixture._root.path().canonicalize().unwrap()))
        .install(&source, "project-summary", "project-summary")
        .unwrap();
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    let mut result = Ok(Output::Plugin(info));
    crate::store::plugins::finish(
        &fixture.database.connection,
        &Command::InstallPluginUpload {
            stream: StreamId::new(),
            source: plugin::UploadSource::Directory,
            name: "project-summary".into(),
            expected_revision: 0,
        },
        &mut result,
    )
    .unwrap();
    let Output::Plugin(info) = result.unwrap() else {
        panic!("package expected")
    };
    info
}

#[test]
fn preserves_snapshots() {
    let mut fixture = setup();
    let provider = install_provider(&mut fixture);
    let role = profile(&mut fixture);
    let first = start(&mut fixture);
    let child = invoke(
        &mut fixture,
        first,
        json!({"role":"review","task":"Initial child task"}),
    )
    .unwrap();
    let original =
        crate::store::commands::session(&fixture.database.connection, child.turn.session).unwrap();
    assert_eq!(original.profile.as_ref().unwrap().provider, provider);
    let mut event = AdkEvent::new("child-history-fixture");
    event.author = "assistant".into();
    event.set_content(adk_core::Content::new("model").with_text("Original child history 中文 🙂"));
    history::append(
        &mut fixture.database,
        child.turn.session,
        child.turn.id,
        event,
        &fixture.events,
    )
    .unwrap();
    finish(&mut fixture, child.turn.id);
    finish(&mut fixture, first);
    let original_history =
        history::pages::read(&fixture.database.connection, child.turn.session, None, 20)
            .unwrap()
            .page;
    let mut changed = provider.clone();
    changed.endpoint = "https://changed.invalid/v1".into();
    changed.default_model = "new-default-model".into();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::PutProvider {
            provider: changed,
            expected_revision: provider.revision,
        },
    );
    let mut parent = super::super::super::super::commands::session(
        &fixture.database.connection,
        fixture.session,
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let worktree = WorktreeId::new();
    super::super::super::super::worktrees::register(
        &fixture.database.connection,
        &Worktree {
            id: worktree,
            project: parent.project,
            path: directory
                .path()
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap()
                .into(),
            main: false,
        },
    )
    .unwrap();
    parent.worktree = worktree;
    parent.config.effort = Effort::High;
    parent.config.model = "new-parent-model".into();
    super::super::super::super::commands::revise_session(&fixture.database.connection, parent)
        .unwrap();
    let package = crate::store::plugins::get(&fixture.database.connection, "files")
        .unwrap()
        .unwrap();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetPluginEnabled {
            name: "files".into(),
            expected_revision: package.summary.revision,
            enabled: false,
        },
    );
    let second = start(&mut fixture);
    let resumed = invoke(
        &mut fixture,
        second,
        json!({"session":child.turn.session,"task":"Continue the child task"}),
    )
    .unwrap();
    assert_eq!(resumed.turn.session, child.turn.session);
    assert_ne!(resumed.turn.id, child.turn.id);
    assert_eq!(resumed.turn.config, child.turn.config);
    assert_eq!(resumed.worktree, child.worktree);
    assert_ne!(resumed.turn.plugins, child.turn.plugins);
    assert!(
        !resumed
            .turn
            .plugins
            .iter()
            .any(|package| package.name == "files")
    );
    assert!(
        child
            .turn
            .plugins
            .iter()
            .any(|package| package.name == "files")
    );
    assert_eq!(
        resumed.turn.plugins,
        super::super::super::plugins::capture(&fixture.database.connection).unwrap()
    );
    assert_eq!(
        super::super::super::plugins::read(&fixture.database.connection, child.turn.id).unwrap(),
        child.turn.plugins
    );
    assert!(
        super::super::super::plugins::packages(&fixture.database.connection, &child.turn.plugins)
            .unwrap()
            .iter()
            .any(|package| package.summary.name == "files")
    );
    assert_eq!(resumed.provider, Some(provider.clone()));
    assert_eq!(resumed.child.as_ref().unwrap().role, Some(role));
    let body: Vec<u8> = fixture
        .database
        .connection
        .query_row(
            "SELECT provider FROM agent_runs WHERE turn=?1",
            [resumed.turn.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(body, encode(&provider).unwrap());
    let db = &fixture.database.connection;
    let current = crate::store::commands::session(db, child.turn.session).unwrap();
    assert_eq!(current.id, original.id);
    assert_eq!(current.project, original.project);
    assert_eq!(current.worktree, original.worktree);
    assert_eq!(current.revision, original.revision);
    assert_eq!(current.config, original.config);
    assert_eq!(current.roles, original.roles);
    assert_eq!(current.profile, original.profile);
    assert_eq!(current.delegation, original.delegation);
    let current_history = history::pages::read(db, child.turn.session, None, 20)
        .unwrap()
        .page;
    assert_eq!(current_history.entries, original_history.entries);
    assert_eq!(current_history.runs[0], original_history.runs[0]);
    assert_eq!(read(db, child.turn.session).unwrap().unwrap().turn, first);
    assert_eq!(origin(db, resumed.turn.id).unwrap().unwrap().turn, second);
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sessions", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    let summaries = fixture.read(None, 20).page.children;
    assert_eq!(
        summaries
            .iter()
            .map(|child| child.run.turn)
            .collect::<Vec<_>>(),
        [child.turn.id, resumed.turn.id]
    );
}

#[test]
fn captures_current_resources() {
    let mut fixture = setup();
    let package = settings_package(&fixture);
    let parent = start(&mut fixture);
    let child = invoke(
        &mut fixture,
        parent,
        json!({"role":null,"task":"Initial task"}),
    )
    .unwrap();
    assert_eq!(
        child.plugin_settings["project-summary"]
            .as_ref()
            .unwrap()
            .values["include_untracked"],
        json!(true)
    );
    finish(&mut fixture, child.turn.id);
    finish(&mut fixture, parent);
    crate::store::agent::tests::fixture::package(
        &fixture.database,
        fixture._root.path(),
        "code-review",
    );
    let Output::PluginSettings(settings) = command(
        &mut fixture.database,
        &fixture.events,
        Command::SavePluginSettings {
            package: package.summary.reference(),
            values: std::collections::BTreeMap::from([("include_untracked".into(), json!(false))]),
            secrets: Default::default(),
        },
    ) else {
        panic!("settings expected")
    };
    assert_eq!(settings.package.settings_revision, 1);
    let parent = start(&mut fixture);
    let resumed = invoke(
        &mut fixture,
        parent,
        json!({"session":child.turn.session,"task":"Updated task"}),
    )
    .unwrap();
    assert_eq!(resumed.turn.session, child.turn.session);
    assert!(resumed.turn.plugins.contains(&settings.package));
    assert!(
        resumed
            .turn
            .plugins
            .iter()
            .any(|package| package.name == "code-review")
    );
    assert!(
        !child
            .turn
            .plugins
            .iter()
            .any(|package| package.name == "code-review")
    );
    assert_eq!(
        resumed.plugin_settings["project-summary"]
            .as_ref()
            .unwrap()
            .values["include_untracked"],
        json!(false)
    );
    let old =
        super::super::super::plugins::packages(&fixture.database.connection, &child.turn.plugins)
            .unwrap();
    let old = old
        .iter()
        .find(|package| package.summary.name == "project-summary")
        .unwrap();
    assert_eq!(
        crate::store::plugins::settings::read_admitted(&fixture.database.connection, old)
            .unwrap()
            .values["include_untracked"],
        json!(true)
    );
    assert_eq!(
        super::super::super::plugins::read(&fixture.database.connection, child.turn.id).unwrap(),
        child.turn.plugins
    );
    finish(&mut fixture, resumed.turn.id);
    finish(&mut fixture, parent);
    let current = crate::store::plugins::get(&fixture.database.connection, "project-summary")
        .unwrap()
        .unwrap();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::RemovePlugin {
            name: "project-summary".into(),
            expected_revision: current.summary.revision,
        },
    );
    let parent = start(&mut fixture);
    let next = invoke(
        &mut fixture,
        parent,
        json!({"session":child.turn.session,"task":"Task after removal"}),
    )
    .unwrap();
    assert_eq!(next.turn.session, child.turn.session);
    assert!(
        !next
            .turn
            .plugins
            .iter()
            .any(|package| package.name == "project-summary")
    );
    assert!(!next.plugin_settings.contains_key("project-summary"));
    for (turn, expected) in [(&child.turn, true), (&resumed.turn, false)] {
        let references =
            super::super::super::plugins::read(&fixture.database.connection, turn.id).unwrap();
        assert_eq!(references, turn.plugins);
        let retained =
            super::super::super::plugins::packages(&fixture.database.connection, &references)
                .unwrap();
        let retained = retained
            .iter()
            .find(|package| package.summary.name == "project-summary")
            .unwrap();
        assert_eq!(
            crate::store::plugins::settings::read_admitted(&fixture.database.connection, retained)
                .unwrap()
                .values["include_untracked"],
            json!(expected)
        );
    }
    assert_eq!(
        fixture
            .database
            .connection
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn keeps_unlisted_provider() {
    let mut fixture = setup();
    let provider = install_provider(&mut fixture);
    let parent = start(&mut fixture);
    let child = invoke(
        &mut fixture,
        parent,
        json!({"role":null,"task":"Initial task"}),
    )
    .unwrap();
    let original =
        crate::store::commands::session(&fixture.database.connection, child.turn.session).unwrap();
    finish(&mut fixture, child.turn.id);
    finish(&mut fixture, parent);
    command(
        &mut fixture.database,
        &fixture.events,
        Command::RemoveProvider {
            provider: provider.id,
            expected_revision: provider.revision,
        },
    );
    assert!(
        crate::store::agent::providers::get(&fixture.database.connection, provider.id)
            .unwrap()
            .is_none()
    );
    let parent = start(&mut fixture);
    let resumed = invoke(
        &mut fixture,
        parent,
        json!({"session":child.turn.session,"task":"Private profile task"}),
    )
    .unwrap();
    assert_eq!(resumed.turn.session, child.turn.session);
    assert_eq!(resumed.turn.config, original.config);
    assert_eq!(resumed.provider, Some(provider));
    assert_eq!(
        crate::store::commands::session(&fixture.database.connection, child.turn.session)
            .unwrap()
            .profile,
        original.profile
    );
    assert_eq!(
        fixture
            .database
            .connection
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn rejects_missing_founding_skill() {
    for remove in [false, true] {
        let mut fixture = setup();
        crate::store::agent::tests::fixture::package(
            &fixture.database,
            fixture._root.path(),
            "code-review",
        );
        let original = profile(&mut fixture);
        let mut role = original.clone();
        role.skills = vec!["code-review:code-review".into()];
        let Output::Role(role) = command(
            &mut fixture.database,
            &fixture.events,
            Command::PutRole {
                role,
                expected_revision: original.revision,
            },
        ) else {
            panic!("role expected")
        };
        command(
            &mut fixture.database,
            &fixture.events,
            Command::SetSessionRoles {
                session: fixture.session,
                expected_revision: 2,
                roles: vec![role.reference()],
            },
        );
        let parent = start(&mut fixture);
        let child = invoke(
            &mut fixture,
            parent,
            json!({"role":"review","task":"Initial review"}),
        )
        .unwrap();
        finish(&mut fixture, child.turn.id);
        finish(&mut fixture, parent);
        let package = crate::store::plugins::get(&fixture.database.connection, "code-review")
            .unwrap()
            .unwrap();
        let change = if remove {
            Command::RemovePlugin {
                name: "code-review".into(),
                expected_revision: package.summary.revision,
            }
        } else {
            Command::SetPluginEnabled {
                name: "code-review".into(),
                expected_revision: package.summary.revision,
                enabled: false,
            }
        };
        command(&mut fixture.database, &fixture.events, change);
        let parent = start(&mut fixture);
        assert_eq!(
            invoke(
                &mut fixture,
                parent,
                json!({"session":child.turn.session,"task":"Unavailable review"})
            )
            .err()
            .unwrap()
            .code,
            ErrorCode::NotConfigured
        );
        let retained = super::super::super::plugins::packages(
            &fixture.database.connection,
            &child.turn.plugins,
        )
        .unwrap();
        assert!(crate::plugins::resources::skill(&retained, "code-review:code-review").is_ok());
        assert_eq!(
            super::super::child(&fixture.database.connection, child.turn.id)
                .unwrap()
                .unwrap()
                .role,
            Some(role)
        );
        assert_eq!(
            fixture
                .database
                .connection
                .query_row("SELECT COUNT(*) FROM agent_delegations", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}

#[test]
fn keeps_founding_role() {
    let mut fixture = setup();
    let original = profile(&mut fixture);
    let parent = start(&mut fixture);
    let child = invoke(
        &mut fixture,
        parent,
        json!({"role":"review","task":"Inspect"}),
    )
    .unwrap();
    finish(&mut fixture, child.turn.id);
    finish(&mut fixture, parent);
    let mut changed = original.clone();
    changed.key = "updated".into();
    changed.name = "Changed review".into();
    changed.instructions = "Changed instructions".into();
    changed.max_turns = Some(1);
    let Output::Role(changed) = command(
        &mut fixture.database,
        &fixture.events,
        Command::PutRole {
            role: changed,
            expected_revision: original.revision,
        },
    ) else {
        panic!("role expected")
    };
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionRoles {
            session: fixture.session,
            expected_revision: 2,
            roles: vec![changed.reference()],
        },
    );
    let parent = start(&mut fixture);
    let resumed = invoke(
        &mut fixture,
        parent,
        json!({"session":child.turn.session,"task":"Continue"}),
    )
    .unwrap();
    assert_eq!(resumed.child.unwrap().role, Some(original.clone()));
    let summary = children::get(
        &fixture.database.connection,
        &runs::get(&fixture.database.connection, resumed.turn.id).unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(summary.name, Some(original.name));
    assert_eq!(summary.origin.role, Some(original.id));
}

#[test]
fn rejects_missing_foreign_and_busy_children() {
    let mut fixture = setup();
    let parent = start(&mut fixture);
    let child = invoke(&mut fixture, parent, json!({"role":null,"task":"Inspect"})).unwrap();
    for (session, code) in [
        (json!(child.turn.session), ErrorCode::Busy),
        (json!(SessionId::new()), ErrorCode::NotFound),
        (json!(fixture.session), ErrorCode::WrongTarget),
        (Value::Null, ErrorCode::InvalidRequest),
    ] {
        assert_eq!(
            invoke(
                &mut fixture,
                parent,
                json!({"session":session,"task":"Resume"})
            )
            .err()
            .unwrap()
            .code,
            code
        );
    }
    finish(&mut fixture, child.turn.id);
    finish(&mut fixture, parent);
    let original = super::super::super::super::commands::session(
        &fixture.database.connection,
        fixture.session,
    )
    .unwrap();
    let Output::Session(other) = command(
        &mut fixture.database,
        &fixture.events,
        Command::CreateSession {
            project: original.project,
            worktree: Some(original.worktree),
            config: Some(original.config),
        },
    ) else {
        panic!("session expected")
    };
    fixture.session = other.id;
    let parent = start(&mut fixture);
    assert_eq!(
        invoke(
            &mut fixture,
            parent,
            json!({"session":child.turn.session,"task":"Resume"})
        )
        .err()
        .unwrap()
        .code,
        ErrorCode::WrongTarget
    );
    assert_eq!(
        fixture
            .database
            .connection
            .query_row("SELECT COUNT(*) FROM agent_delegations", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn rejects_changed_role_or_scope() {
    let mut fixture = setup();
    profile(&mut fixture);
    let parent = start(&mut fixture);
    let child = invoke(
        &mut fixture,
        parent,
        json!({"role":"review","task":"Inspect"}),
    )
    .unwrap();
    finish(&mut fixture, child.turn.id);
    for role in [Value::Null, json!("missing")] {
        let args = json!({"session":child.turn.session,"role":role,"task":"Resume"});
        assert!(invoke(&mut fixture, parent, args).is_err());
    }
    let location = tempfile::tempdir().unwrap();
    let scope = WorktreeId::new();
    super::super::super::super::worktrees::register(
        &fixture.database.connection,
        &Worktree {
            id: scope,
            project: child.project,
            path: location
                .path()
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap()
                .into(),
            main: false,
        },
    )
    .unwrap();
    assert_eq!(
        invoke(
            &mut fixture,
            parent,
            json!({"session":child.turn.session,"worktree":scope,"task":"Resume"})
        )
        .err()
        .unwrap()
        .code,
        ErrorCode::Conflict
    );
}

#[test]
fn bounds_parent_permissions() {
    for (permission, mode) in [
        (Permission::Ask, WorkMode::Code),
        (Permission::Full, WorkMode::Plan),
    ] {
        let mut fixture = setup();
        let mut session = super::super::super::super::commands::session(
            &fixture.database.connection,
            fixture.session,
        )
        .unwrap();
        session.config.permission = Permission::Full;
        super::super::super::super::commands::revise_session(&fixture.database.connection, session)
            .unwrap();
        let parent = start(&mut fixture);
        let child = invoke(&mut fixture, parent, json!({"role":null,"task":"Inspect"})).unwrap();
        finish(&mut fixture, child.turn.id);
        finish(&mut fixture, parent);
        let mut session = super::super::super::super::commands::session(
            &fixture.database.connection,
            fixture.session,
        )
        .unwrap();
        session.config.permission = permission;
        session.config.mode = mode;
        super::super::super::super::commands::revise_session(&fixture.database.connection, session)
            .unwrap();
        let parent = start(&mut fixture);
        assert_eq!(
            invoke(
                &mut fixture,
                parent,
                json!({"session":child.turn.session,"task":"Resume"})
            )
            .err()
            .unwrap()
            .code,
            ErrorCode::PermissionDenied
        );
    }
}
