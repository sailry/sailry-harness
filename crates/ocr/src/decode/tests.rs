use super::*;
use ndarray::Array3;

#[test]
fn preserves_recognition_contracts() {
    for (scenario, check) in [
        (
            "decodes_tokens_and_blank_outputs",
            (|scenario: &'static str| {
                for (case, tokens, columns, selected, expected, confidence) in [
                    (
                        "Unicode",
                        "A\n🇨🇳\n中\n",
                        5,
                        vec![1, 1, 0, 1, 2, 3, 4],
                        "AA🇨🇳中 ",
                        0.95,
                    ),
                    ("blank", "A\n", 3, vec![0, 0], "", 0.0),
                ] {
                    let dictionary = Dictionary::parse(tokens).unwrap();
                    let mut output = Array3::zeros((1, selected.len(), columns));
                    for (row, token) in selected.into_iter().enumerate() {
                        output[[0, row, token]] = if case == "blank" { 1.0 } else { 0.95 };
                    }
                    let decoded = dictionary.decode(&output.into_dyn()).unwrap();
                    if case == "blank" {
                        assert_eq!(decoded, (String::new(), 0.0), "{scenario}: {case}");
                    } else {
                        assert_eq!(decoded.0, expected, "{scenario}: {case}: text");
                        assert!(
                            (decoded.1 - confidence).abs() < 1e-6,
                            "{scenario}: {case}: confidence"
                        );
                    }
                }
            }) as fn(&'static str),
        ),
        (
            "rejects_mismatched_vocabulary_and_probabilities",
            (|scenario: &'static str| {
                let dictionary = Dictionary::parse("A\nB\n").unwrap();
                assert!(
                    dictionary
                        .decode(&Array3::zeros((1, 2, 3)).into_dyn())
                        .is_err(),
                    "{scenario}"
                );
                let mut output = Array3::zeros((1, 2, 4));
                output[[0, 0, 1]] = f32::NAN;
                assert!(
                    dictionary.decode(&output.clone().into_dyn()).is_err(),
                    "{scenario}"
                );
                output[[0, 0, 1]] = -1.0;
                assert!(dictionary.decode(&output.into_dyn()).is_err(), "{scenario}");
            }),
        ),
        (
            "rejects_empty_or_control_entries",
            (|scenario: &'static str| {
                for text in ["", "A\n\nB", "A\n\t\n", "A\0\n"] {
                    assert!(Dictionary::parse(text).is_err(), "{scenario}: {text:?}");
                }
                assert_eq!(
                    Dictionary::parse("A\r\nB\r\n").unwrap().len(),
                    4,
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
