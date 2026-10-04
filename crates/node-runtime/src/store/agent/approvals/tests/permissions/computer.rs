use super::*;

fn name(read_only: bool) -> String {
    crate::computer::catalog()
        .as_array()
        .unwrap()
        .iter()
        .find_map(|tool| {
            let name = tool["name"].as_str()?;
            (crate::computer::read_only(name) == Some(read_only)).then(|| name.to_owned())
        })
        .unwrap()
}

fn capture(fixture: &Fixture) -> plugin::Info {
    let db = &fixture.database.connection;
    let mut package = crate::store::plugins::get(db, "files").unwrap().unwrap();
    package.summary.name = "native-tools".into();
    package.summary.digest = blake3::hash(b"native-tools-test").to_hex().to_string();
    package.summary.settings_revision = 0;
    package.settings = None;
    let extension = package.extension.as_mut().unwrap();
    extension.actions = vec![
        plugin::Action::ReadComputer,
        plugin::Action::ControlComputer,
    ];
    extension.tools.clear();
    extension.settings_schema = None;
    extension.desktop = Some(
        serde_json::from_value(json!({"conversations":[{
            "id":"native", "resource":"workspace", "context":"",
            "tools":[{"kind":"plugin", "name":name(true)}]
        }]}))
        .unwrap(),
    );
    db.execute(
        "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)",
        params![
            package.summary.digest,
            package.summary.name,
            encode(&package).unwrap()
        ],
    )
    .unwrap();
    let mut references = crate::store::agent::plugins::read(db, fixture.turn).unwrap();
    references.push(package.summary.reference());
    db.execute(
        "UPDATE turns SET plugins=?1 WHERE id=?2",
        params![encode(&references).unwrap(), fixture.turn.to_string()],
    )
    .unwrap();
    package
}

#[test]
fn uses_captured_native_authority() {
    for permission in [Permission::Ask, Permission::Project, Permission::Full] {
        let fixture = Fixture::with_permission(permission);
        let db = &fixture.database.connection;
        let mut request = fixture.request.clone();
        request.tool_name = name(false);
        assert_eq!(
            crate::store::agent::permissions::source(db, fixture.turn, &request).unwrap(),
            ApprovalSource::User,
        );
        capture(&fixture);
        assert_eq!(
            crate::store::agent::permissions::source(db, fixture.turn, &request).unwrap(),
            if permission == Permission::Full {
                ApprovalSource::Full
            } else {
                ApprovalSource::User
            },
        );
        request.tool_name = "computer_click".into();
        assert_eq!(
            crate::store::agent::permissions::source(db, fixture.turn, &request).unwrap(),
            ApprovalSource::User,
        );
    }
}

#[test]
fn preserves_planning_and_exact_selection() {
    for mode in [WorkMode::Code, WorkMode::Plan] {
        let fixture = Fixture::with_mode(Permission::Full, mode);
        let package = capture(&fixture);
        let db = &fixture.database.connection;
        let session = crate::store::commands::session(db, fixture.session).unwrap();
        let context = plugin::Context {
            invocation: None,
            turn: Some(fixture.turn),
            surface: Default::default(),
            package: package.summary.reference(),
            worktree: Some(session.worktree),
            session: Some(fixture.session),
        };
        assert!(
            crate::store::agent::plugins::check_computer(db, &context, &package, &name(true))
                .is_ok()
        );
        let mutation =
            crate::store::agent::plugins::check_computer(db, &context, &package, &name(false));
        if mode == WorkMode::Plan {
            assert_eq!(mutation.unwrap_err().code, ErrorCode::PermissionDenied);
        } else {
            mutation.unwrap();
        }
        let mut config = crate::store::agent::permissions::config(db, fixture.turn).unwrap();
        config.assistant = Some(plugin::conversation::Binding {
            package: package.summary.reference(),
            id: "native".into(),
        });
        db.execute(
            "UPDATE session_revisions SET config=?1 WHERE session=?2 AND revision=1",
            params![encode(&config).unwrap(), fixture.session.to_string()],
        )
        .unwrap();
        assert_eq!(
            crate::store::agent::plugins::operation(db, fixture.turn, &name(true)).unwrap(),
            Some(sailry_protocol::tool::Operation::ReadComputer),
        );
        assert_eq!(
            crate::store::agent::plugins::operation(db, fixture.turn, &name(false)).unwrap(),
            None,
        );
        assert!(
            crate::store::agent::plugins::check_computer(db, &context, &package, &name(true))
                .is_ok()
        );
        assert_eq!(
            crate::store::agent::plugins::check_computer(db, &context, &package, &name(false))
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied,
        );
    }
}
