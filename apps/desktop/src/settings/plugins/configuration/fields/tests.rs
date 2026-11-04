use super::*;
use core::prelude::v1::test;

#[test]
fn numeric_steps() {
    for (scenario, check) in [
        (
            "steps_exact_integers",
            (|_scenario: &str| {
                assert_eq!(
                    stepped("9007199254740993", StepAction::Increment).as_deref(),
                    Some("9007199254740994"),
                    "steps exact integers"
                );
                assert_eq!(
                    stepped("9007199254740993", StepAction::Decrement).as_deref(),
                    Some("9007199254740992"),
                    "steps exact integers"
                );
                assert_eq!(
                    stepped("18446744073709551615", StepAction::Increment),
                    None,
                    "steps exact integers"
                );
                assert_eq!(
                    stepped("-9223372036854775808", StepAction::Decrement),
                    None,
                    "steps exact integers"
                );
            }) as fn(&str),
        ),
        (
            "steps_empty_and_fractional_values",
            (|_scenario: &str| {
                assert_eq!(
                    stepped("", StepAction::Increment).as_deref(),
                    Some("1"),
                    "steps empty and fractional values"
                );
                assert_eq!(
                    stepped(" ", StepAction::Decrement).as_deref(),
                    Some("-1"),
                    "steps empty and fractional values"
                );
                assert_eq!(
                    stepped("1.5", StepAction::Increment).as_deref(),
                    Some("2.5"),
                    "steps empty and fractional values"
                );
                assert_eq!(
                    stepped("invalid", StepAction::Increment),
                    None,
                    "steps empty and fractional values"
                );
                assert_eq!(
                    stepped("true", StepAction::Increment),
                    None,
                    "steps empty and fractional values"
                );
                assert_eq!(
                    stepped("1e999", StepAction::Increment),
                    None,
                    "steps empty and fractional values"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
