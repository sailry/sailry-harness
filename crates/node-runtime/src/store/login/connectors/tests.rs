use super::*;

#[test]
fn caps_reported_deadlines() {
    for reported in [10, 30] {
        assert_eq!(
            prompt(
                login::State::Browser {
                    authorization_url: "https://example.com/authorize".into(),
                    expires_at_ms: reported,
                },
                20,
            ),
            LoginState::Browser {
                url: "https://example.com/authorize".into(),
                expires_at_ms: reported.min(20),
            }
        );
        assert_eq!(
            prompt(
                login::State::Pending {
                    verification_url: "https://example.com/device".into(),
                    user_code: "ABCD".into(),
                    expires_at_ms: reported,
                },
                20,
            ),
            LoginState::Device {
                url: "https://example.com/device".into(),
                code: "ABCD".into(),
                expires_at_ms: reported.min(20),
            }
        );
    }
}

#[tokio::test(start_paused = true)]
async fn bounds_abandoned_browser_flow() {
    let started = Instant::now();
    let deadline = started + crate::providers::login::DEADLINE;
    let (sender, receiver) = oneshot::channel();
    sender.send(deadline + Duration::from_secs(60)).unwrap();
    expire(deadline, receiver).await;
    assert_eq!(Instant::now(), deadline);
}

#[tokio::test(start_paused = true)]
async fn honors_earlier_device_expiry() {
    let started = Instant::now();
    let deadline = started + crate::providers::login::DEADLINE;
    let expected = started + Duration::from_secs(2);
    let (sender, receiver) = oneshot::channel();
    sender.send(expected).unwrap();
    expire(deadline, receiver).await;
    assert_eq!(Instant::now(), expected);
}

#[tokio::test(start_paused = true)]
async fn bounds_flow_without_prompt() {
    for closed in [false, true] {
        let deadline = Instant::now() + crate::providers::login::DEADLINE;
        let (sender, receiver) = oneshot::channel();
        let sender = (!closed).then_some(sender);
        expire(deadline, receiver).await;
        assert_eq!(Instant::now(), deadline);
        drop(sender);
    }
}
