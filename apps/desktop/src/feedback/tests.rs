use super::Scope;

#[test]
fn outcomes_and_recovery() {
    let mut scope = Scope::default();
    assert_eq!(
        scope.reconcile(vec!["failed"]),
        (vec!["failed"], vec![]),
        "new failure"
    );
    scope.record("failed", false);
    assert_eq!(
        scope.reconcile(vec!["failed"]),
        (vec![], vec![]),
        "unchanged failure"
    );
    assert_eq!(scope.reconcile(vec![]), (vec![], vec![]), "rapid clear");
    assert!(scope.release().is_empty(), "outcome survives clear");
    assert_eq!(
        scope.reconcile(vec!["failed"]),
        (vec!["failed"], vec![]),
        "failure recurs"
    );
    scope.reconcile(vec![]);
    scope.reconcile(vec!["retry"]);
    scope.record("retry", true);
    assert_eq!(
        scope.reconcile(vec![]),
        (vec![], vec!["retry"]),
        "recovered condition"
    );
    assert!(scope.release().is_empty(), "recovery already released");
    scope.reconcile(vec!["outcome", "retry"]);
    scope.record("outcome", false);
    scope.record("retry", true);
    assert_eq!(scope.release(), vec!["retry"], "only recovery revoked");
    assert!(scope.release().is_empty(), "release once");
}
