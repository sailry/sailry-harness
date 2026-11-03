use super::*;

#[test]
fn bounds_batch_count() {
    assert!(matches!(
        validate_inputs(&[]),
        Err(InputError::Empty { .. })
    ));
    let mut inputs = vec![Input::Focus { focused: true }; MAX_INPUT_BATCH];
    assert!(validate_inputs(&inputs).is_ok());
    inputs.push(Input::Focus { focused: false });
    assert!(matches!(
        validate_inputs(&inputs),
        Err(InputError::TooMany { max: MAX_INPUT_BATCH, actual, .. }) if actual == MAX_INPUT_BATCH + 1
    ));
}

#[test]
fn bounds_combined_text_bytes() {
    let mut inputs = vec![
        Input::Text {
            text: "a".repeat(MAX_INPUT_BYTES / 2),
        },
        Input::Paste {
            text: "é".repeat(MAX_INPUT_BYTES / 4),
        },
    ];
    assert!(validate_inputs(&inputs).is_ok());
    inputs.push(Input::Text { text: "a".into() });
    assert!(matches!(
        validate_inputs(&inputs),
        Err(InputError::TooLarge { max: MAX_INPUT_BYTES, actual, .. }) if actual == MAX_INPUT_BYTES + 1
    ));
}

#[test]
fn rejects_invalid_tail() {
    let inputs = [
        Input::Text {
            text: "valid".into(),
        },
        Input::Text { text: "\n".into() },
    ];
    assert!(matches!(
        validate_inputs(&inputs),
        Err(InputError::Invalid { .. })
    ));
}
