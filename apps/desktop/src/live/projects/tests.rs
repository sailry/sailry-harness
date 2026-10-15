use super::*;
use core::prelude::v1::test;

#[test]
fn distinguishes_task_and_queue_busy() {
    let mut error = Fault::new(ErrorCode::Busy, "fixture queue is full");
    assert_eq!(failure(&error), ("project_remove_busy", false));
    error.details = Some(FaultDetails::ProjectRemoval {
        tasks_active: true,
        terminals_may_be_closed: false,
    });
    assert_eq!(failure(&error), ("project_remove_tasks", false));
    error.details = Some(FaultDetails::ProjectRemoval {
        tasks_active: true,
        terminals_may_be_closed: true,
    });
    assert_eq!(failure(&error), ("project_remove_tasks_partial", false));
}

#[test]
fn distinguishes_cleanup_and_transport_failures() {
    for code in [ErrorCode::NotFound, ErrorCode::Unavailable] {
        let mut error = Fault::new(code, "fixture terminal is unavailable");
        error.details = Some(FaultDetails::ProjectRemoval {
            tasks_active: false,
            terminals_may_be_closed: true,
        });
        assert_eq!(failure(&error), ("project_remove_failed", false));
        error.details = None;
        assert_eq!(
            failure(&error),
            if code == ErrorCode::Unavailable {
                ("project_remove_unknown", true)
            } else {
                ("project_changed", false)
            },
        );
    }
}

#[test]
fn preserves_uncertain_identity() {
    let mut error = Fault::new(ErrorCode::OutcomeUnknown, "fixture result was not recorded");
    error.details = Some(FaultDetails::ProjectRemoval {
        tasks_active: false,
        terminals_may_be_closed: true,
    });
    assert_eq!(failure(&error), ("project_remove_unknown", true));
    error.code = ErrorCode::RevisionConflict;
    assert_eq!(failure(&error), ("project_remove_changed_partial", false));
}
