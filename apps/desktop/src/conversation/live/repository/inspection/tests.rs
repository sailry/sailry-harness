use super::*;

fn scope() -> Option<(NodeId, WorktreeId)> {
    Some((NodeId([1; 32]), WorktreeId::new()))
}

#[test]
fn flights_and_recovery() {
    for (scenario, check) in [
        (
            "failed_read_remains_unknown_and_can_recover",
            (|_scenario: &str| {
                let mut inspection = Inspection::default();
                inspection.select(scope(), true);
                let first = inspection.begin(false).unwrap();
                assert!(
                    inspection.begin(false).is_none(),
                    "failed read remains unknown and can recover"
                );
                assert!(
                    inspection.finish(first, None),
                    "failed read remains unknown and can recover"
                );
                assert_eq!(
                    inspection.kind(),
                    Kind::Unknown,
                    "failed read remains unknown and can recover"
                );
                assert!(
                    inspection.report_failure(),
                    "failed read remains unknown and can recover"
                );
                assert!(
                    !inspection.report_failure(),
                    "failed read remains unknown and can recover"
                );
                assert!(
                    inspection.begin(false).is_none(),
                    "failed read remains unknown and can recover"
                );
                let second = inspection.begin(true).unwrap();
                assert!(
                    inspection.finish(second, Some(RepositoryKind::Ready)),
                    "failed read remains unknown and can recover"
                );
                assert_eq!(
                    inspection.kind(),
                    Kind::Repository,
                    "failed read remains unknown and can recover"
                );
                assert!(
                    inspection.begin(false).is_none(),
                    "failed read remains unknown and can recover"
                );
            }) as fn(&str),
        ),
        (
            "only_success_can_classify_plain_directories",
            (|_scenario: &str| {
                let mut inspection = Inspection::default();
                inspection.select(scope(), true);
                let first = inspection.begin(false).unwrap();
                assert_eq!(
                    inspection.kind(),
                    Kind::Unknown,
                    "only success can classify plain directories"
                );
                inspection.finish(first, Some(RepositoryKind::Directory));
                assert_eq!(
                    inspection.kind(),
                    Kind::PlainDirectory,
                    "only success can classify plain directories"
                );
                let refresh = inspection.begin(true).unwrap();
                inspection.finish(refresh, None);
                assert_eq!(
                    inspection.kind(),
                    Kind::Unknown,
                    "only success can classify plain directories"
                );
            }),
        ),
        (
            "reconnect_discards_old_flights_and_rechecks",
            (|_scenario: &str| {
                let mut inspection = Inspection::default();
                let target = scope();
                inspection.select(target, true);
                let old = inspection.begin(false).unwrap();
                inspection.select(target, false);
                assert!(
                    inspection.begin(true).is_none(),
                    "reconnect discards old flights and rechecks"
                );
                inspection.select(target, true);
                let new = inspection.begin(false).unwrap();
                assert!(
                    !inspection.finish(old, Some(RepositoryKind::Directory)),
                    "reconnect discards old flights and rechecks"
                );
                assert!(
                    inspection.finish(new, Some(RepositoryKind::Ready)),
                    "reconnect discards old flights and rechecks"
                );
                inspection.select(target, false);
                inspection.select(target, true);
                assert!(
                    inspection.begin(false).is_some(),
                    "reconnect discards old flights and rechecks"
                );
            }),
        ),
        (
            "switching_back_rejects_the_original_response",
            (|_scenario: &str| {
                let mut inspection = Inspection::default();
                let first = scope();
                inspection.select(first, true);
                let stale = inspection.begin(false).unwrap();
                inspection.select(scope(), true);
                inspection.select(first, true);
                let current = inspection.begin(false).unwrap();
                assert!(
                    !inspection.finish(stale, Some(RepositoryKind::Directory)),
                    "switching back rejects the original response"
                );
                assert_eq!(
                    inspection.kind(),
                    Kind::Unknown,
                    "switching back rejects the original response"
                );
                assert!(
                    inspection.finish(current, Some(RepositoryKind::Ready)),
                    "switching back rejects the original response"
                );
            }),
        ),
        (
            "coalesces_pending_invalidations",
            (|_scenario: &str| {
                let mut inspection = Inspection::default();
                inspection.select(scope(), true);
                let first = inspection.begin(false).unwrap();
                for _ in 0..3 {
                    assert!(
                        inspection.begin(true).is_none(),
                        "coalesces pending invalidations"
                    );
                }
                assert!(
                    inspection.finish(first, Some(RepositoryKind::Ready)),
                    "coalesces pending invalidations"
                );
                let follow_up = inspection.begin(false).unwrap();
                assert!(
                    inspection.begin(false).is_none(),
                    "coalesces pending invalidations"
                );
                assert!(
                    inspection.finish(follow_up, Some(RepositoryKind::Ready)),
                    "coalesces pending invalidations"
                );
                assert!(
                    inspection.begin(false).is_none(),
                    "coalesces pending invalidations"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
