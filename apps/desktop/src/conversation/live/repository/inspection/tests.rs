use super::*;

fn scope() -> Option<(NodeId, WorktreeId)> {
    Some((NodeId([1; 32]), WorktreeId::new()))
}

#[test]
fn failed_read_remains_unknown_and_can_recover() {
    let mut inspection = Inspection::default();
    inspection.select(scope(), true);
    let first = inspection.begin(false).unwrap();
    assert!(inspection.begin(false).is_none());
    assert!(inspection.finish(first, None));
    assert_eq!(inspection.kind(), Kind::Unknown);
    assert!(inspection.report_failure());
    assert!(!inspection.report_failure());
    assert!(inspection.begin(false).is_none());
    let second = inspection.begin(true).unwrap();
    assert!(inspection.finish(second, Some(RepositoryKind::Ready)));
    assert_eq!(inspection.kind(), Kind::Repository);
    assert!(inspection.begin(false).is_none());
}

#[test]
fn only_success_can_classify_plain_directories() {
    let mut inspection = Inspection::default();
    inspection.select(scope(), true);
    let first = inspection.begin(false).unwrap();
    assert_eq!(inspection.kind(), Kind::Unknown);
    inspection.finish(first, Some(RepositoryKind::Directory));
    assert_eq!(inspection.kind(), Kind::PlainDirectory);
    let refresh = inspection.begin(true).unwrap();
    inspection.finish(refresh, None);
    assert_eq!(inspection.kind(), Kind::Unknown);
}

#[test]
fn reconnect_discards_old_flights_and_rechecks() {
    let mut inspection = Inspection::default();
    let target = scope();
    inspection.select(target, true);
    let old = inspection.begin(false).unwrap();
    inspection.select(target, false);
    assert!(inspection.begin(true).is_none());
    inspection.select(target, true);
    let new = inspection.begin(false).unwrap();
    assert!(!inspection.finish(old, Some(RepositoryKind::Directory)));
    assert!(inspection.finish(new, Some(RepositoryKind::Ready)));
    inspection.select(target, false);
    inspection.select(target, true);
    assert!(inspection.begin(false).is_some());
}

#[test]
fn switching_back_rejects_the_original_response() {
    let mut inspection = Inspection::default();
    let first = scope();
    inspection.select(first, true);
    let stale = inspection.begin(false).unwrap();
    inspection.select(scope(), true);
    inspection.select(first, true);
    let current = inspection.begin(false).unwrap();
    assert!(!inspection.finish(stale, Some(RepositoryKind::Directory)));
    assert_eq!(inspection.kind(), Kind::Unknown);
    assert!(inspection.finish(current, Some(RepositoryKind::Ready)));
}

#[test]
fn coalesces_pending_invalidations() {
    let mut inspection = Inspection::default();
    inspection.select(scope(), true);
    let first = inspection.begin(false).unwrap();
    for _ in 0..3 {
        assert!(inspection.begin(true).is_none());
    }
    assert!(inspection.finish(first, Some(RepositoryKind::Ready)));
    let follow_up = inspection.begin(false).unwrap();
    assert!(inspection.begin(false).is_none());
    assert!(inspection.finish(follow_up, Some(RepositoryKind::Ready)));
    assert!(inspection.begin(false).is_none());
}
