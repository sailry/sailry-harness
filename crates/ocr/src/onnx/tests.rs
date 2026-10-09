use super::*;

#[test]
fn checks_real_outputs_after_dynamic_metadata() {
    assert!(validate_shape(Role::Detector, &[-1, -1, -1, -1], true).is_ok());
    assert!(validate_shape(Role::Detector, &[1, 1, 64, 96], false).is_ok());
    for shape in [[1, 3, 64, 96], [1, 1, 0, 96], [1, 1, 64, 4096]] {
        assert!(validate_shape(Role::Detector, &shape, false).is_err());
    }
    assert!(validate_shape(Role::Recognizer(6906), &[-1, -1, 6906], true).is_ok());
    assert!(validate_shape(Role::Recognizer(6906), &[1, 40, 18385], false).is_err());
    assert!(validate_shape(Role::Recognizer(18385), &[1, 40, 18385], false).is_ok());
    assert!(validate_shape(Role::Orientation, &[1, 2], false).is_ok());
    assert!(validate_shape(Role::Orientation, &[1, 4], false).is_err());
}
