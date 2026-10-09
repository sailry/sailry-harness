use super::*;
use ndarray::Array3;

#[test]
fn retains_complete_unicode_tokens() {
    let dictionary = Dictionary::parse("A\n🇨🇳\n中\n").unwrap();
    let mut output = Array3::zeros((1, 7, 5));
    for (row, token) in [1, 1, 0, 1, 2, 3, 4].into_iter().enumerate() {
        output[[0, row, token]] = 0.95;
    }
    let (text, confidence) = dictionary.decode(&output.into_dyn()).unwrap();
    assert_eq!(text, "AA🇨🇳中 ");
    assert!((confidence - 0.95).abs() < 1e-6);
}

#[test]
fn rejects_mismatched_vocabulary_and_probabilities() {
    let dictionary = Dictionary::parse("A\nB\n").unwrap();
    assert!(
        dictionary
            .decode(&Array3::zeros((1, 2, 3)).into_dyn())
            .is_err()
    );
    let mut output = Array3::zeros((1, 2, 4));
    output[[0, 0, 1]] = f32::NAN;
    assert!(dictionary.decode(&output.clone().into_dyn()).is_err());
    output[[0, 0, 1]] = -1.0;
    assert!(dictionary.decode(&output.into_dyn()).is_err());
}

#[test]
fn rejects_empty_or_control_entries() {
    for text in ["", "A\n\nB", "A\n\t\n", "A\0\n"] {
        assert!(Dictionary::parse(text).is_err(), "{text:?}");
    }
    assert_eq!(Dictionary::parse("A\r\nB\r\n").unwrap().len(), 4);
}

#[test]
fn returns_empty_for_blank_output() {
    let dictionary = Dictionary::parse("A\n").unwrap();
    let mut output = Array3::zeros((1, 2, 3));
    output[[0, 0, 0]] = 1.0;
    output[[0, 1, 0]] = 1.0;
    assert_eq!(
        dictionary.decode(&output.into_dyn()).unwrap(),
        (String::new(), 0.0)
    );
}
