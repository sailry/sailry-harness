use super::*;
use crate::Apply;

#[test]
fn preserves_order_on_reconnect() {
    let attempt = login::Attempt {
        id: RequestId::new(),
        provider: ProviderId::new(),
    };
    let mut projection = Projection::new(attempt.clone());
    let update = |revision, state| {
        Update::ProviderLogin(login::Update {
            attempt: attempt.clone(),
            revision,
            state,
        })
    };
    projection.reconnect(1).unwrap();
    assert_eq!(
        projection
            .apply(1, update(2, login::State::Starting))
            .unwrap(),
        Apply::Applied
    );
    projection.reconnect(2).unwrap();
    assert_eq!(
        projection
            .apply(1, update(4, login::State::Connected))
            .unwrap(),
        Apply::Ignored
    );
    assert_eq!(
        projection
            .apply(2, update(1, login::State::Starting))
            .unwrap(),
        Apply::Ignored
    );
    assert_eq!(
        projection
            .apply(2, update(2, login::State::Starting))
            .unwrap(),
        Apply::Ignored
    );
    assert!(
        projection
            .apply(2, update(2, login::State::Cancelled))
            .is_err()
    );
    assert_eq!(
        projection
            .apply(2, update(5, login::State::Connected))
            .unwrap(),
        Apply::Applied
    );
    assert!(
        projection
            .apply(2, update(6, login::State::Starting))
            .is_err()
    );
    assert!(projection.reconnect(2).is_err());
    assert!(
        projection
            .apply(
                2,
                Update::ProviderLogin(login::Update {
                    attempt: login::Attempt {
                        id: RequestId::new(),
                        provider: attempt.provider
                    },
                    revision: 6,
                    state: login::State::Connected
                })
            )
            .is_err()
    );
}
