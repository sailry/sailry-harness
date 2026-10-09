use super::*;
use sailry_protocol::process::Access;
mod settings;

fn request(fixture: &mut Fixture, access: Access) -> Binding {
    request_with_access(fixture, Some(access))
}

fn request_with_access(fixture: &mut Fixture, access: Option<Access>) -> Binding {
    super::super::super::tests::fixture::package(
        &fixture.database,
        fixture._directory.path(),
        "commands",
    );
    let package = crate::store::plugins::get(&fixture.database.connection, "commands")
        .unwrap()
        .unwrap();
    let mut references =
        crate::store::agent::plugins::read(&fixture.database.connection, fixture.turn).unwrap();
    references.push(package.summary.reference());
    fixture
        .database
        .connection
        .execute(
            "UPDATE turns SET plugins=?1 WHERE id=?2",
            params![encode(&references).unwrap(), fixture.turn.to_string()],
        )
        .unwrap();
    fixture.request.tool_name = crate::plugins::tools::alias("commands", "run_command");
    fixture.request.args = json!({"command":"true"});
    if let Some(access) = access {
        fixture.request.args["access"] = json!(access);
    }
    fixture.request.function_call_id = Some("command-access".into());
    let mut event = AdkEvent::new("command-access");
    event.author = "assistant".into();
    event.set_content(adk_core::Content {
        role: "model".into(),
        parts: vec![adk_core::Part::FunctionCall {
            id: fixture.request.function_call_id.clone(),
            name: fixture.request.tool_name.clone(),
            args: fixture.request.args.clone(),
            thought_signature: None,
        }],
    });
    history::append(
        &mut fixture.database,
        fixture.session,
        fixture.turn,
        event,
        &fixture.events,
    )
    .unwrap();
    Binding {
        owner: package.summary.reference(),
        tool: fixture.request.tool_name.clone(),
        operation: Some(sailry_protocol::tool::Operation::RunCommand),
    }
}

fn prepared(turn: TurnId, access: Access) -> Command {
    Command::RunCommand {
        turn,
        command: "true".into(),
        access,
        description: None,
        cwd: String::new(),
        timeout_ms: 1000,
        pty: false,
        background: false,
        attachments: vec![],
    }
}

#[test]
fn distinguishes_confined_and_unrestricted_authority() {
    for permission in [Permission::Ask, Permission::Project, Permission::Full] {
        for access in [Access::Workspace, Access::Unrestricted] {
            let mut fixture = Fixture::with_permission(permission);
            let binding = request(&mut fixture, access);
            let approval = begin(
                &mut fixture.database,
                fixture.turn,
                &fixture.request,
                Some(&binding),
                &fixture.events,
            )
            .unwrap();
            assert_eq!(approval.access, Some(access));
            if access == Access::Unrestricted && permission == Permission::Full {
                assert_eq!(approval.source, ApprovalSource::Full);
                assert_eq!(approval.state, ApprovalState::Approved);
                assert!(!approval.can_remember);
                assert_eq!(approval.prefix, None);
                assert_eq!(approval.choice, None);
            } else if access == Access::Unrestricted && crate::process::sandbox::SUPPORTED {
                assert_eq!(approval.source, ApprovalSource::User);
                assert_eq!(approval.state, ApprovalState::Pending);
                assert!(!approval.can_remember);
                assert_eq!(approval.prefix, None);
                for decision in [
                    Decision::ApproveSession,
                    Decision::ApprovePrefix("true".into()),
                ] {
                    assert_eq!(
                        resolve(
                            &fixture.database.connection,
                            fixture.session,
                            approval.id,
                            decision
                        )
                        .unwrap_err()
                        .code,
                        ErrorCode::InvalidRequest
                    );
                }
                fixture.approve(approval.id);
            } else if crate::process::sandbox::SUPPORTED {
                assert_eq!(approval.source, ApprovalSource::Sandbox);
                assert_eq!(approval.state, ApprovalState::Approved);
                assert!(!approval.can_remember);
                assert_eq!(approval.prefix, None);
            } else if approval.state == ApprovalState::Pending {
                fixture.approve(approval.id);
            }
            authorize(
                &mut fixture.database.connection,
                fixture.turn,
                &fixture.request,
                Some(&prepared(fixture.turn, access)),
            )
            .unwrap();
        }
    }
}

#[test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn stale_rules_cannot_authorize_native_escape() {
    for (source, choice, access) in [
        (
            ApprovalSource::SessionRule,
            Some(ApprovalChoice::Session),
            Some(Access::Unrestricted),
        ),
        (
            ApprovalSource::PrefixRule,
            Some(ApprovalChoice::Prefix("true".into())),
            Some(Access::Unrestricted),
        ),
        (ApprovalSource::Full, None, Some(Access::Workspace)),
        (ApprovalSource::Full, None, None),
        (
            ApprovalSource::User,
            Some(ApprovalChoice::Once),
            Some(Access::Workspace),
        ),
        (ApprovalSource::User, Some(ApprovalChoice::Once), None),
    ] {
        let mut fixture = Fixture::new();
        let mut approval = fixture.begin();
        fixture.approve(approval.id);
        approval.state = ApprovalState::Approved;
        approval.source = source;
        approval.choice = choice;
        approval.access = access;
        put(&fixture.database.connection, &approval).unwrap();
        assert_eq!(
            authorize(
                &mut fixture.database.connection,
                fixture.turn,
                &fixture.request,
                Some(&prepared(fixture.turn, Access::Unrestricted))
            )
            .unwrap_err()
            .code,
            ErrorCode::PermissionDenied
        );
        let claimed: bool = fixture
            .database
            .connection
            .query_row(
                "SELECT claimed FROM agent_approvals WHERE id=?1",
                [approval.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!claimed);
    }
}

#[test]
fn unrestricted_access_uses_admitted_permission() {
    for (admitted, current, source, state) in [
        (
            Permission::Full,
            Permission::Ask,
            ApprovalSource::Full,
            ApprovalState::Approved,
        ),
        (
            Permission::Ask,
            Permission::Full,
            ApprovalSource::User,
            ApprovalState::Pending,
        ),
        (
            Permission::Project,
            Permission::Full,
            ApprovalSource::User,
            ApprovalState::Pending,
        ),
    ] {
        let mut fixture = Fixture::with_permission(admitted);
        let binding = request(&mut fixture, Access::Unrestricted);
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
        let approval = begin(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            Some(&binding),
            &fixture.events,
        )
        .unwrap();
        assert_eq!(approval.source, source);
        assert_eq!(approval.state, state);
        assert_eq!(approval.access, Some(Access::Unrestricted));
        assert!(!approval.can_remember || !crate::process::sandbox::SUPPORTED);
        if state == ApprovalState::Pending {
            fixture.approve(approval.id);
        }
        authorize(
            &mut fixture.database.connection,
            fixture.turn,
            &fixture.request,
            Some(&prepared(fixture.turn, Access::Unrestricted)),
        )
        .unwrap();
        assert!(
            authorize(
                &mut fixture.database.connection,
                fixture.turn,
                &fixture.request,
                Some(&prepared(fixture.turn, Access::Unrestricted)),
            )
            .is_err()
        );
    }
}

#[test]
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn preserves_existing_command_approval() {
    for permission in [Permission::Ask, Permission::Project, Permission::Full] {
        for access in [Access::Workspace, Access::Unrestricted] {
            let mut fixture = Fixture::with_permission(permission);
            let binding = request(&mut fixture, access);
            let approval = begin(
                &mut fixture.database,
                fixture.turn,
                &fixture.request,
                Some(&binding),
                &fixture.events,
            )
            .unwrap();
            assert_eq!(approval.access, Some(access));
            if permission == Permission::Full {
                assert_eq!(approval.source, ApprovalSource::Full);
                assert_eq!(approval.state, ApprovalState::Approved);
            } else {
                assert_eq!(approval.source, ApprovalSource::User);
                assert_eq!(approval.state, ApprovalState::Pending);
                assert!(approval.can_remember);
                fixture.approve(approval.id);
            }
            authorize(
                &mut fixture.database.connection,
                fixture.turn,
                &fixture.request,
                Some(&prepared(fixture.turn, access)),
            )
            .unwrap();
        }
    }
}

#[test]
fn planning_denies_both_access_levels() {
    for permission in [Permission::Ask, Permission::Project, Permission::Full] {
        for access in [Access::Workspace, Access::Unrestricted] {
            let mut fixture = Fixture::with_mode(permission, WorkMode::Plan);
            let binding = request(&mut fixture, access);
            assert_eq!(
                begin(
                    &mut fixture.database,
                    fixture.turn,
                    &fixture.request,
                    Some(&binding),
                    &fixture.events
                )
                .unwrap_err()
                .code,
                ErrorCode::PermissionDenied
            );
            let approvals: i64 = fixture
                .database
                .connection
                .query_row("SELECT count(*) FROM agent_approvals", [], |row| row.get(0))
                .unwrap();
            assert_eq!(approvals, 0);
        }
    }
}
