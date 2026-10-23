use super::Scope;

#[test]
fn rapid_clear_preserves_outcome() {
    let mut scope = Scope::default();
    assert_eq!(scope.reconcile(vec!["failed"]), (vec!["failed"], vec![]));
    scope.record("failed", false);
    assert_eq!(scope.reconcile(vec![]), (vec![], vec![]));
    assert!(scope.release().is_empty());
}

#[test]
fn unchanged_error_does_not_repeat() {
    let mut scope = Scope::default();
    scope.reconcile(vec!["failed"]);
    scope.record("failed", false);
    // Dismissing the toast does not change the retained application error.
    assert_eq!(scope.reconcile(vec!["failed"]), (vec![], vec![]));
    scope.reconcile(vec![]);
    assert_eq!(scope.reconcile(vec!["failed"]), (vec!["failed"], vec![]));
}

#[test]
fn recovery_ends_with_condition() {
    let mut scope = Scope::default();
    scope.reconcile(vec!["retry"]);
    scope.record("retry", true);
    assert_eq!(scope.reconcile(vec![]), (vec![], vec!["retry"]));
    assert!(scope.release().is_empty());
}

#[test]
fn release_revokes_only_recovery() {
    let mut scope = Scope::default();
    scope.reconcile(vec!["outcome", "retry"]);
    scope.record("outcome", false);
    scope.record("retry", true);
    assert_eq!(scope.release(), vec!["retry"]);
    assert!(scope.release().is_empty());
}
