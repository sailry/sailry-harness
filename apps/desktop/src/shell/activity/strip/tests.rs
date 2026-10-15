use core::prelude::v1::test;
use std::time::Duration;

#[test]
fn rotation_pulses_once_then_pauses_each_second() {
    let animation = crate::ui::identicon::rotation();
    assert_eq!(animation.duration, Duration::from_secs(1));
    assert!(!animation.oneshot);
    for (phase, rotation) in [(0., 0.), (0.1125, 0.15625), (0.225, 0.5), (0.3375, 0.84375)] {
        assert!(((animation.easing)(phase) - rotation).abs() < 1e-6);
    }
    for phase in [0.45, 0.5, 0.75, 1.] {
        assert_eq!((animation.easing)(phase), 1.);
    }
}
