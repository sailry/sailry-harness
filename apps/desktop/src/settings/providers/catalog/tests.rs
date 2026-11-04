use super::*;
use core::prelude::v1::test;

#[test]
fn refresh_outcomes() {
    for (scenario, check) in [
        (
            "keeps_failed_automatic_refreshes_quiet",
            (|_scenario: &str| {
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
                assert_eq!(
                    state.status, status,
                    "keeps failed automatic refreshes quiet"
                );
                assert!(
                    state.error.is_some(),
                    "keeps failed automatic refreshes quiet: retry must remain available"
                );
                assert_eq!(
                    state.feedback(),
                    None,
                    "keeps failed automatic refreshes quiet"
                );

                state.complete(Source::Manual, Err(ErrorCode::Busy));
                assert_eq!(
                    state.status, status,
                    "keeps failed automatic refreshes quiet"
                );
                assert_eq!(
                    state.feedback(),
                    Some("provider_catalog_busy"),
                    "keeps failed automatic refreshes quiet"
                );
                state.complete(Source::Manual, Err(ErrorCode::Unavailable));
                assert_eq!(
                    state.feedback(),
                    Some("provider_catalog_failed"),
                    "keeps failed automatic refreshes quiet"
                );

                let updated = catalog::Status {
                    revision: 4,
                    models: 10,
                    ..Default::default()
                };
                state.complete(Source::Manual, Ok(updated.clone()));
                assert_eq!(
                    state.status, updated,
                    "keeps failed automatic refreshes quiet"
                );
                assert!(
                    state.error.is_none(),
                    "keeps failed automatic refreshes quiet"
                );
                assert_eq!(
                    state.feedback(),
                    None,
                    "keeps failed automatic refreshes quiet"
                );
            }) as fn(&str),
        ),
        (
            "preserves_newer_catalog_observations",
            (|_scenario: &str| {
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
                assert_eq!(
                    state.status.revision, 5,
                    "preserves newer catalog observations"
                );
                assert!(
                    state.error.is_none(),
                    "preserves newer catalog observations"
                );
                assert_eq!(
                    state.feedback(),
                    None,
                    "preserves newer catalog observations"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
