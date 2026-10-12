use super::*;

fn sample(minor: u32) -> Value {
    json!({"nbformat":4,"nbformat_minor":minor,"metadata":{"custom":"kept"},"cells":[
        {"cell_type":"markdown","metadata":{},"source":"Notes"},
        {"cell_type":"code","metadata":{},"source":["print(1)\n"],"outputs":[],"execution_count":null},
        {"cell_type":"raw","id":"retained-id","metadata":{},"source":"Raw"}
    ]})
}

#[test]
fn fills_only_missing_current_ids() {
    let value = sample(5);
    let before = value.to_string();
    let after = prepare("analysis.ipynb", &before).unwrap();
    assert_eq!(after, prepare("analysis.ipynb", &before).unwrap());
    let result = validation::parse(&after).unwrap();
    let ids: BTreeSet<_> = result["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| cell["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 3);
    assert_eq!(result["cells"][2], value["cells"][2]);
    for index in 0..2 {
        let mut original = value["cells"][index].clone();
        original["id"] = result["cells"][index]["id"].clone();
        assert_eq!(result["cells"][index], original);
    }
    assert_eq!(result["metadata"], value["metadata"]);
    assert!(matches!(
        prepare("analysis.ipynb", &after).unwrap(),
        Cow::Borrowed(_)
    ));
}

#[test]
fn preserves_legacy_and_ordinary_text() {
    let legacy = sample(4).to_string();
    assert_eq!(prepare("analysis.ipynb", &legacy).unwrap(), legacy);
    assert!(validation::parse(&legacy).is_ok());
    assert_eq!(prepare("notes.txt", "not JSON").unwrap(), "not JSON");
}

#[test]
fn rejects_malformed_and_duplicate_ids() {
    assert!(prepare("analysis.ipynb", "not JSON").is_err());
    let mut value = sample(5);
    value["cells"][0]["id"] = json!("retained-id");
    assert!(prepare("analysis.ipynb", &value.to_string()).is_err());
    value["cells"][0]["id"] = json!("invalid id");
    assert!(prepare("analysis.ipynb", &value.to_string()).is_err());
    value["cells"][0].as_object_mut().unwrap().remove("id");
    value["cells"][1]["outputs"] = json!({});
    assert!(prepare("analysis.ipynb", &value.to_string()).is_err());
}
