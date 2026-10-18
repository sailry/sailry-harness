use super::*;

#[test]
fn retains_more_than_a_fixed_item_count() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    for number in 0..129 {
        std::fs::write(directory.path().join(format!("item-{number:03}")), []).unwrap();
    }
    std::fs::write(directory.path().join("pending.crdownload"), []).unwrap();
    let result = listing(&root, sailry_protocol::NodeId([1; 32])).unwrap();
    assert_eq!(result["files"].as_array().unwrap().len(), 129);
    assert_eq!(result["truncated"], false);
    assert!(serde_json::to_vec(&result).unwrap().len() <= RESULT_BYTES);
}

#[test]
fn reports_the_actual_output_budget() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    for number in 0..600 {
        std::fs::write(
            directory
                .path()
                .join(format!("item-{number:03}-{}", "x".repeat(100))),
            [],
        )
        .unwrap();
    }
    let result = listing(&root, sailry_protocol::NodeId([1; 32])).unwrap();
    assert_eq!(result["truncated"], true);
    let files = result["files"].as_array().unwrap();
    assert!(files.len() > 128 && files.len() < 600);
    assert!(serde_json::to_vec(&result).unwrap().len() <= RESULT_BYTES);
    assert!(result.get("next").is_none());
}
