use super::*;

#[test]
fn reports_cancellation_before_work() {
    assert!(matches!(check(&|| true), Err(Error::Cancelled)));
    assert!(check(&|| false).is_ok());
}

#[path = "tests/reference.rs"]
mod reference;
