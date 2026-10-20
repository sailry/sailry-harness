use super::*;
use core::prelude::v1::test;

#[test]
fn keeps_failed_automatic_refreshes_quiet() {
    let status = catalog::Status {
        revision: 3,
        models: 9,
        ..Default::default()
    };
    let mut state = State {
        status: status.clone(),
        ..Default::default()
    };
    state.complete(Source::Automatic, Err(ErrorCode::Unavailable));
    assert_eq!(state.status, status);
    assert!(state.error.is_some(), "retry must remain available");
    assert_eq!(state.feedback(), None);

    state.complete(Source::Manual, Err(ErrorCode::Busy));
    assert_eq!(state.status, status);
    assert_eq!(state.feedback(), Some("provider_catalog_busy"));
    state.complete(Source::Manual, Err(ErrorCode::Unavailable));
    assert_eq!(state.feedback(), Some("provider_catalog_failed"));

    let updated = catalog::Status {
        revision: 4,
        models: 10,
        ..Default::default()
    };
    state.complete(Source::Manual, Ok(updated.clone()));
    assert_eq!(state.status, updated);
    assert!(state.error.is_none());
    assert_eq!(state.feedback(), None);
}

#[test]
fn preserves_newer_catalog_observations() {
    let mut state = State::default();
    state.status.revision = 5;
    state.complete(Source::Automatic, Err(ErrorCode::Unavailable));
    state.complete(
        Source::Automatic,
        Ok(catalog::Status {
            revision: 4,
            ..Default::default()
        }),
    );
    assert_eq!(state.status.revision, 5);
    assert!(state.error.is_none());
    assert_eq!(state.feedback(), None);
}
