use super::*;
use core::prelude::v1::test;

#[test]
fn does_not_treat_denial_as_expiration() {
    assert_eq!(
        error_key(&Fault::new(ErrorCode::Expired, "Expired fixture")),
        "provider_login_expired"
    );
    assert_eq!(
        error_key(&Fault::new(ErrorCode::PermissionDenied, "Denied fixture")),
        "provider_login_failed"
    );
}
