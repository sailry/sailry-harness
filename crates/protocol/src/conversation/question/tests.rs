use super::*;

#[test]
fn defaults_other_answers_to_disabled() {
    let value = serde_json::json!({"prompt":"Pick a target", "input":{
        "kind":"choice", "options":["one", "two"], "multiple":false
    }});
    let spec: Spec = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(spec, choice(false, false));
    spec.validate_answer(&Answer::Choices {
        selected: vec![0],
        other: None,
    })
    .unwrap();
    assert!(
        spec.validate_answer(&Answer::Choices {
            selected: vec![],
            other: Some("Another answer".into()),
        })
        .is_err()
    );
    for invalid in [
        serde_json::json!({"kind":"choice", "options":["One"]}),
        serde_json::json!({"kind":"choice", "multiple":false}),
        serde_json::json!({"kind":"choice", "options":["One"], "multiple":false, "allow_other":null}),
        serde_json::json!({"kind":"choice", "options":["One"], "multiple":false, "unknown":false}),
        serde_json::json!({"kind":"text", "max_bytes":512}),
    ] {
        let mut invalid_spec = value.clone();
        invalid_spec["input"] = invalid;
        assert!(serde_json::from_value::<Spec>(invalid_spec).is_err());
    }
}

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

#[test]
fn reserves_empty_prompts_for_plans() {
    for input in [
        Input::Plan,
        Input::Text {
            multiline: true,
            max_bytes: 64,
        },
        Input::Choice {
            options: vec!["one".into()],
            multiple: false,
            allow_other: false,
        },
        Input::Form { fields: vec![] },
        Input::Url {
            url: "https://example.invalid/continue".into(),
            elicitation_id: "request".into(),
        },
    ] {
        let mut spec = Spec {
            prompt: "Review".into(),
            input,
        };
        spec.validate().unwrap();
        for prompt in ["", " ", "\n\t"] {
            spec.prompt = prompt.into();
            assert_eq!(
                spec.validate().is_ok(),
                prompt.is_empty() && matches!(spec.input, Input::Plan),
                "unexpected validation for {spec:?}"
            );
        }
    }
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
    for options in [vec![], vec!["".into()], vec!["same".into(), "same".into()]] {
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

#[test]
fn preserves_long_prompts_and_complete_choices() {
    let mut options = (0..65)
        .map(|number| format!("Option {number}"))
        .collect::<Vec<_>>();
    options[0] = "x".repeat(1025);
    let spec = Spec {
        prompt: "p".repeat(80 * 1024),
        input: Input::Choice {
            options,
            multiple: false,
            allow_other: false,
        },
    };
    spec.validate_answer(&Answer::Choices {
        selected: vec![64],
        other: None,
    })
    .unwrap();
    assert_eq!(
        serde_json::from_value::<Spec>(serde_json::to_value(&spec).unwrap()).unwrap(),
        spec
    );
    assert!(
        spec.validate_answer(&Answer::Choices {
            selected: vec![65],
            other: None
        })
        .is_err()
    );
}

#[test]
fn forms_use_the_existing_complete_payload_budget() {
    let mut fields = (0..65)
        .map(|number| form::Field {
            name: format!("field-{number}"),
            title: "Title".into(),
            description: None,
            required: false,
            input: form::Input::Boolean,
        })
        .collect::<Vec<_>>();
    fields[0].name = "n".repeat(1025);
    fields[0].title = "t".repeat(1025);
    let values = fields
        .iter()
        .map(|field| (field.name.clone(), serde_json::json!(true)))
        .collect();
    let spec = Spec {
        prompt: "Form".into(),
        input: Input::Form {
            fields: fields.clone(),
        },
    };
    spec.validate_answer(&Answer::Form(values)).unwrap();
    fields[0].title = "t".repeat(MAX_TEXT_BYTES);
    assert!(
        Spec {
            input: Input::Form { fields },
            ..spec
        }
        .validate()
        .is_err()
    );
}
