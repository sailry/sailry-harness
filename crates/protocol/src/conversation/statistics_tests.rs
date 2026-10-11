use super::*;

#[test]
fn defaults_missing_delegated_usage() {
    let mut wire = serde_json::to_value(Statistics::default()).unwrap();
    wire.as_object_mut().unwrap().remove("delegated");
    assert_eq!(
        serde_json::from_value::<Statistics>(wire).unwrap(),
        Statistics::default()
    );
}
