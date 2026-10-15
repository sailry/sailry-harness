use super::*;
use core::prelude::v1::test;

#[test]
fn steps_exact_integers() {
    assert_eq!(
        stepped("9007199254740993", StepAction::Increment).as_deref(),
        Some("9007199254740994")
    );
    assert_eq!(
        stepped("9007199254740993", StepAction::Decrement).as_deref(),
        Some("9007199254740992")
    );
    assert_eq!(stepped("18446744073709551615", StepAction::Increment), None);
    assert_eq!(stepped("-9223372036854775808", StepAction::Decrement), None);
}

#[test]
fn steps_empty_and_fractional_values() {
    assert_eq!(stepped("", StepAction::Increment).as_deref(), Some("1"));
    assert_eq!(stepped(" ", StepAction::Decrement).as_deref(), Some("-1"));
    assert_eq!(
        stepped("1.5", StepAction::Increment).as_deref(),
        Some("2.5")
    );
    assert_eq!(stepped("invalid", StepAction::Increment), None);
    assert_eq!(stepped("true", StepAction::Increment), None);
    assert_eq!(stepped("1e999", StepAction::Increment), None);
}
