use super::*;

#[test]
fn rejects_non_web_destinations() {
    let spec = |url: &str| Spec {
        prompt: "Continue in the service".into(),
        input: Input::Url {
            url: url.into(),
            elicitation_id: "request".into(),
        },
    };
    for url in [
        "https://example.invalid/continue?task=1#step",
        "http://127.0.0.1:4321/",
        "http://[::1]:4321/",
    ] {
        spec(url).validate_answer(&Answer::Opened).unwrap();
        assert!(
            spec(url)
                .validate_answer(&Answer::Text("confirmed".into()))
                .is_err()
        );
    }
    for url in [
        "file:///tmp/fixture",
        "javascript:alert(1)",
        "https://user@example.invalid/",
        "https://@example.invalid/",
        "http://example.invalid/",
        "https://example.invalid/\n",
    ] {
        assert!(
            spec(url).validate().is_err(),
            "invalid destination accepted: {url}"
        );
    }
    assert!(
        choice(false, false)
            .validate_answer(&Answer::Opened)
            .is_err()
    );
}

#[test]
fn validates_plan_feedback() {
    let spec = Spec {
        prompt: "Review the plan 中文 🙂".into(),
        input: Input::Plan,
    };
    spec.validate().unwrap();
    for feedback in ["", "  ", "Add tests 中文 🙂\nThen review"] {
        spec.validate_answer(&Answer::Text(feedback.into()))
            .unwrap();
    }
    assert!(
        spec.validate_answer(&Answer::Text("x".repeat(MAX_TEXT_BYTES + 1)))
            .is_err()
    );
    assert!(
        spec.validate_answer(&Answer::Plan {
            turn: TurnId::new()
        })
        .is_err()
    );
    let response = Response::StartCoding {
        expected_revision: 7,
        message: "Implement the accepted plan".into(),
    };
    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(value["kind"], "start_coding");
    assert_eq!(value["data"]["expected_revision"], 7);
    assert_eq!(serde_json::from_value::<Response>(value).unwrap(), response);
}

fn choice(multiple: bool, allow_other: bool) -> Spec {
    Spec {
        prompt: "Pick a target".into(),
        input: Input::Choice {
            options: vec!["one".into(), "two".into()],
            multiple,
            allow_other,
        },
    }
}

#[test]
fn preserves_text_within_limits() {
    let mut spec = Spec {
        prompt: "Describe the task".into(),
        input: Input::Text {
            multiline: true,
            max_bytes: 11,
        },
    };
    let answer = Answer::Text(" 中文🙂 ".into());
    assert!(spec.validate_answer(&answer).is_err());
    spec.input = Input::Text {
        multiline: false,
        max_bytes: 12,
    };
    spec.validate_answer(&answer).unwrap();
    for text in ["", "  ", "a\nb", "a\rb"] {
        assert!(spec.validate_answer(&Answer::Text(text.into())).is_err());
    }
    spec.input = Input::Text {
        multiline: true,
        max_bytes: 12,
    };
    spec.validate_answer(&Answer::Text("a\nb".into())).unwrap();
    let encoded = serde_json::to_value(&answer).unwrap();
    assert_eq!(serde_json::from_value::<Answer>(encoded).unwrap(), answer);
}

#[test]
fn validates_choices() {
    for multiple in [false, true] {
        for allow_other in [false, true] {
            let spec = choice(multiple, allow_other);
            spec.validate().unwrap();
            spec.validate_answer(&Answer::Choices {
                selected: vec![1],
                other: None,
            })
            .unwrap();
            for selected in [vec![], vec![2], vec![0, 0]] {
                assert!(
                    spec.validate_answer(&Answer::Choices {
                        selected,
                        other: None
                    })
                    .is_err()
                );
            }
            assert_eq!(
                spec.validate_answer(&Answer::Choices {
                    selected: vec![1, 0],
                    other: None
                })
                .is_ok(),
                multiple
            );
            assert_eq!(
                spec.validate_answer(&Answer::Choices {
                    selected: vec![],
                    other: Some("custom".into())
                })
                .is_ok(),
                allow_other
            );
            assert_eq!(
                spec.validate_answer(&Answer::Choices {
                    selected: vec![0],
                    other: Some("custom".into())
                })
                .is_ok(),
                multiple && allow_other
            );
            assert!(
                spec.validate_answer(&Answer::Choices {
                    selected: vec![],
                    other: Some(" ".into())
                })
                .is_err()
            );
            assert!(spec.validate_answer(&Answer::Text("one".into())).is_err());
        }
    }
}

#[test]
fn rejects_invalid_specs() {
    for options in [
        vec![],
        vec!["".into()],
        vec!["same".into(), "same".into()],
        vec!["x".repeat(1025)],
        (0..65).map(|index| index.to_string()).collect(),
    ] {
        let spec = Spec {
            prompt: "Pick".into(),
            input: Input::Choice {
                options,
                multiple: true,
                allow_other: true,
            },
        };
        assert!(spec.validate().is_err());
    }
    for max_bytes in [0, MAX_TEXT_BYTES + 1] {
        assert!(
            Spec {
                prompt: "Describe".into(),
                input: Input::Text {
                    multiline: true,
                    max_bytes
                }
            }
            .validate()
            .is_err()
        );
    }
    let secret = serde_json::json!({"prompt": "secret", "input": {"kind": "text", "multiline": false, "max_bytes": 100, "secret": true}});
    assert!(serde_json::from_value::<Spec>(secret).is_err());
}
