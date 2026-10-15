use super::*;
use core::prelude::v1::test;

#[test]
fn distinguishes_confirmed_and_unknown_failures() {
    let error = Fault::new(ErrorCode::Unavailable, "plugin service is unavailable");
    assert_eq!(completion(&Err(error.clone()), false), Err(error.clone()));
    assert_eq!(error_key(&error, false), "plugins_failed");
    assert_eq!(
        completion(&Err(error.clone()), true).unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(error_key(&error, true), "plugins_unknown");
}
