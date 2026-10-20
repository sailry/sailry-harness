use super::*;
use serde_json::json;

fn request() -> Request {
    serde_json::from_value(json!({"state":"Page text", "questions":{
        "visible":{"type":"noul", "instructions":"Is Save visible?"},
        "action":{"type":"choice", "instructions":"Choose an action", "criteria":{"save":null,"cancel":"Discard"}},
        "urgency":{"type":"score", "instructions":"How urgent?", "criteria":["Later","Now"]}
    }})).unwrap()
}

#[test]
fn preserves_typed_questions() {
    let request = request();
    request.validate().unwrap();
    let encoded = serde_json::to_value(&request).unwrap();
    assert_eq!(
        encoded["questions"]["action"]["criteria"]["save"],
        serde_json::Value::Null
    );
    assert_eq!(serde_json::from_value::<Request>(encoded).unwrap(), request);
}

#[test]
fn bounds_input() {
    let mut input = request();
    input.state = "x".repeat(MAX_STATE_BYTES + 1);
    assert!(input.validate().is_err());
    input.state.clear();
    input.questions = (0..MAX_QUESTIONS + 1)
        .map(|index| {
            (
                index.to_string(),
                Question::Noul {
                    instructions: "Question".into(),
                    criteria: BTreeMap::new(),
                },
            )
        })
        .collect();
    assert!(input.validate().is_err());
    input.questions.clear();
    assert!(input.validate().is_err());
}

#[test]
fn rejects_invalid_criteria() {
    let mut input = request();
    input.questions.insert(
        "invalid".into(),
        Question::Score {
            instructions: "Question".into(),
            criteria: vec!["Only one".into()],
        },
    );
    assert!(input.validate().is_err());
    input.questions.insert(
        "invalid".into(),
        Question::Noul {
            instructions: "Question".into(),
            criteria: BTreeMap::from([("maybe".into(), "Unknown".into())]),
        },
    );
    assert!(input.validate().is_err());
}

#[test]
fn accepts_each_input_boundary() {
    let mut input = request();
    input.state = "x".repeat(MAX_STATE_BYTES);
    input.model = Some("a".repeat(128));
    input.questions = (0..MAX_QUESTIONS)
        .map(|index| {
            (
                format!("{index:0>128}"),
                Question::Choice {
                    instructions: "x".repeat(MAX_INSTRUCTIONS_BYTES),
                    criteria: (0..MAX_OPTIONS)
                        .map(|option| {
                            (
                                format!("{option:0>256}"),
                                Some("x".repeat(MAX_OPTION_BYTES)),
                            )
                        })
                        .collect(),
                },
            )
        })
        .collect();
    input.validate().unwrap();
}

#[test]
fn rejects_each_text_overflow() {
    for index in 0..6 {
        let mut input = request();
        match index {
            0 => input.model = Some("a".repeat(129)),
            1 => {
                input.questions = BTreeMap::from([(
                    "q".repeat(129),
                    Question::Noul {
                        instructions: "Q".into(),
                        criteria: BTreeMap::new(),
                    },
                )]);
            }
            2 => {
                input.questions.insert(
                    "q".into(),
                    Question::Noul {
                        instructions: "x".repeat(MAX_INSTRUCTIONS_BYTES + 1),
                        criteria: BTreeMap::new(),
                    },
                );
            }
            3 => {
                input.questions.insert(
                    "q".into(),
                    Question::Noul {
                        instructions: "Q".into(),
                        criteria: BTreeMap::from([(
                            "true".into(),
                            "x".repeat(MAX_OPTION_BYTES + 1),
                        )]),
                    },
                );
            }
            4 => {
                input.questions.insert(
                    "q".into(),
                    Question::Choice {
                        instructions: "Q".into(),
                        criteria: BTreeMap::from([("x".repeat(MAX_OPTION_BYTES + 1), None)]),
                    },
                );
            }
            _ => {
                input.questions.insert(
                    "q".into(),
                    Question::Score {
                        instructions: "Q".into(),
                        criteria: vec!["x".repeat(MAX_OPTION_BYTES + 1), "Other".into()],
                    },
                );
            }
        }
        assert!(input.validate().is_err(), "accepted overflow {index}");
    }
    let mut input = request();
    input.questions.insert(
        "q".into(),
        Question::Choice {
            instructions: "Q".into(),
            criteria: (0..MAX_OPTIONS + 1)
                .map(|index| (index.to_string(), None))
                .collect(),
        },
    );
    assert!(input.validate().is_err());
}

#[test]
fn bounds_unicode_bytes_and_control_characters() {
    let mut input = request();
    input.state = "界".repeat(MAX_STATE_BYTES / 3);
    input.state.push_str(&"x".repeat(MAX_STATE_BYTES % 3));
    assert_eq!(input.state.len(), MAX_STATE_BYTES);
    input.validate().unwrap();
    input.state.push('界');
    assert!(input.validate().is_err());
    input.state = "Page\0text".into();
    assert!(input.validate().is_err());
    input.state.clear();
    input.questions.insert(
        "q".into(),
        Question::Noul {
            instructions: " \n\t".into(),
            criteria: BTreeMap::new(),
        },
    );
    assert!(input.validate().is_err());
}

#[test]
fn preserves_binary_probability_and_rejects_unknown_fields() {
    let mut value = serde_json::to_value(request()).unwrap();
    value["questions"]["visible"]["extra"] = json!(true);
    assert!(serde_json::from_value::<Request>(value).is_err());
    assert_eq!(
        serde_json::to_value(Answer::Noul { noul: 0.75 }).unwrap(),
        json!({"type":"noul","noul":0.75})
    );
}
