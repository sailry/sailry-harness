use super::*;
use serde_json::json;

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "preserves_simple_decisions",
            (|scenario: &'static str| {
                for (decision, wire) in [(Decision::Approve, "approve"), (Decision::Deny, "deny")] {
                    assert_eq!(
                        serde_json::to_value(&decision).unwrap(),
                        json!(wire),
                        "{scenario}"
                    );
                    assert_eq!(
                        serde_json::from_value::<Decision>(json!(wire)).unwrap(),
                        decision,
                        "{scenario}"
                    );
                }
            }) as fn(&'static str),
        ),
        (
            "carries_choices_and_reasons",
            (|scenario: &'static str| {
                for (decision, wire) in [
                    (Decision::ApproveSession, json!("approve_session")),
                    (
                        Decision::ApprovePrefix("cargo test".into()),
                        json!({"approve_prefix":"cargo test"}),
                    ),
                    (
                        Decision::DenyWithReason("Keep the fixture".into()),
                        json!({"deny_with_reason":"Keep the fixture"}),
                    ),
                ] {
                    assert_eq!(serde_json::to_value(&decision).unwrap(), wire, "{scenario}");
                    assert_eq!(
                        serde_json::from_value::<Decision>(wire).unwrap(),
                        decision,
                        "{scenario}"
                    );
                }
            }),
        ),
        (
            "rejects_missing_eligibility",
            (|scenario: &'static str| {
                let value = json!({
                    "id":crate::ApprovalId::new(),"session":SessionId::new(),"turn":TurnId::new(),
                    "entry":"entry","index":0,"state":"pending","source":"user",
                    "choice":null,"reason":null,"prefix":null
                });
                assert!(
                    serde_json::from_value::<Approval>(value).is_err(),
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
