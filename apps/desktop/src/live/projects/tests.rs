use super::*;
use core::prelude::v1::test;

#[test]
fn failures_and_uncertainty() {
    for (scenario, check) in [
        (
            "distinguishes_task_and_queue_busy",
            (|_scenario: &str| {
                let mut error = Fault::new(ErrorCode::Busy, "fixture queue is full");
                assert_eq!(
                    failure(&error),
                    ("project_remove_busy", false),
                    "distinguishes task and queue busy"
                );
                error.details = Some(FaultDetails::ProjectRemoval {
                    tasks_active: true,
                    terminals_may_be_closed: false,
                });
                assert_eq!(
                    failure(&error),
                    ("project_remove_tasks", false),
                    "distinguishes task and queue busy"
                );
                error.details = Some(FaultDetails::ProjectRemoval {
                    tasks_active: true,
                    terminals_may_be_closed: true,
                });
                assert_eq!(
                    failure(&error),
                    ("project_remove_tasks_partial", false),
                    "distinguishes task and queue busy"
                );
            }) as fn(&str),
        ),
        (
            "distinguishes_cleanup_and_transport_failures",
            (|_scenario: &str| {
                for code in [ErrorCode::NotFound, ErrorCode::Unavailable] {
                    let mut error = Fault::new(code, "fixture terminal is unavailable");
                    error.details = Some(FaultDetails::ProjectRemoval {
                        tasks_active: false,
                        terminals_may_be_closed: true,
                    });
                    assert_eq!(
                        failure(&error),
                        ("project_remove_failed", false),
                        "distinguishes cleanup and transport failures"
                    );
                    error.details = None;
                    assert_eq!(
                        failure(&error),
                        if code == ErrorCode::Unavailable {
                            ("project_remove_unknown", true)
                        } else {
                            ("project_changed", false)
                        },
                        "distinguishes cleanup and transport failures"
                    );
                }
            }),
        ),
        (
            "preserves_uncertain_identity",
            (|_scenario: &str| {
                let mut error =
                    Fault::new(ErrorCode::OutcomeUnknown, "fixture result was not recorded");
                error.details = Some(FaultDetails::ProjectRemoval {
                    tasks_active: false,
                    terminals_may_be_closed: true,
                });
                assert_eq!(
                    failure(&error),
                    ("project_remove_unknown", true),
                    "preserves uncertain identity"
                );
                error.code = ErrorCode::RevisionConflict;
                assert_eq!(
                    failure(&error),
                    ("project_remove_changed_partial", false),
                    "preserves uncertain identity"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
