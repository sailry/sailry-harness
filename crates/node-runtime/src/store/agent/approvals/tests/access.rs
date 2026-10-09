use super::*;
use sailry_protocol::process::Access;

fn request(fixture: &mut Fixture, access: Access) -> Binding {
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
    fixture.request.args = json!({"command":"true","access":access});
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
        background: false,
        attachments: vec![],
    }
}

#[test]
fn confinement_is_automatic_and_escape_is_fresh() {
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
            if access == Access::Unrestricted && crate::process::sandbox::SUPPORTED {
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
        (ApprovalSource::Full, None, Some(Access::Unrestricted)),
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
