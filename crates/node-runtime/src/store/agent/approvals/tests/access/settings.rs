use super::*;

fn toggle(fixture: &Fixture, enabled: bool) {
    let mut settings = crate::store::sandbox::read(&fixture.database.connection).unwrap();
    settings.enabled = enabled;
    crate::store::sandbox::save(&fixture.database.connection, &settings).unwrap();
}

#[test]
fn captures_current_policy_without_changing_arguments() {
    for permission in [Permission::Ask, Permission::Project, Permission::Full] {
        let mut fixture = Fixture::with_permission(permission);
        let binding = request_with_access(&mut fixture, None);
        let original = fixture.request.args.clone();
        toggle(&fixture, false);
        let approval = begin(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            Some(&binding),
            &fixture.events,
        )
        .unwrap();
        assert_eq!(approval.access, Some(Access::Unrestricted));
        assert_eq!(fixture.request.args, original);
        if permission == Permission::Full {
            assert_eq!(approval.source, ApprovalSource::Full);
            assert_eq!(approval.state, ApprovalState::Approved);
        } else {
            assert_eq!(approval.source, ApprovalSource::User);
            assert_eq!(approval.state, ApprovalState::Pending);
            fixture.approve(approval.id);
        }
        assert_eq!(
            command_access(&fixture.database.connection, fixture.turn, &fixture.request).unwrap(),
            Access::Unrestricted
        );
        authorize(
            &mut fixture.database.connection,
            fixture.turn,
            &fixture.request,
            Some(&prepared(fixture.turn, Access::Unrestricted)),
        )
        .unwrap();
    }
}

#[test]
fn toggling_preserves_exact_approved_access() {
    for initially_enabled in [false, true] {
        let mut fixture = Fixture::new();
        let binding = request_with_access(&mut fixture, None);
        toggle(&fixture, initially_enabled);
        let approval = begin(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            Some(&binding),
            &fixture.events,
        )
        .unwrap();
        if approval.state == ApprovalState::Pending {
            // Toggle while this request is waiting for the user.
            toggle(&fixture, !initially_enabled);
            fixture.approve(approval.id);
        } else {
            toggle(&fixture, !initially_enabled);
        }
        let access = if initially_enabled {
            Access::Workspace
        } else {
            Access::Unrestricted
        };
        let changed = if initially_enabled {
            Access::Unrestricted
        } else {
            Access::Workspace
        };
        assert_eq!(
            command_access(&fixture.database.connection, fixture.turn, &fixture.request).unwrap(),
            access
        );
        assert_eq!(
            authorize(
                &mut fixture.database.connection,
                fixture.turn,
                &fixture.request,
                Some(&prepared(fixture.turn, changed))
            )
            .unwrap_err()
            .code,
            ErrorCode::PermissionDenied
        );
        authorize(
            &mut fixture.database.connection,
            fixture.turn,
            &fixture.request,
            Some(&prepared(fixture.turn, access)),
        )
        .unwrap();
    }
}
