use super::*;

fn sample() -> Value {
    json!({"nbformat":4,"nbformat_minor":5,"metadata":{"kernelspec":{"language":"python"},"custom":{"kept":true}},"extra":{"kept":true},"cells":[
        {"cell_type":"markdown","id":"intro","metadata":{"tags":["title"]},"source":["# Notebook\n","Text"]},
        {"cell_type":"code","id":"calculation","metadata":{"collapsed":false},"source":"print(1)","outputs":[{"output_type":"stream","name":"stdout","text":["1\n"]}],"execution_count":7,"custom":"retained"},
        {"cell_type":"raw","id":"closing","metadata":{},"source":"Untouched"}
    ]})
}

fn request(before: &str, edits: Vec<notebook::Change>) -> notebook::Edit {
    notebook::Edit {
        path: "sample.ipynb".into(),
        expected_revision: blake3::hash(before.as_bytes()).to_hex().to_string(),
        edits,
    }
}

#[test]
fn preserves_metadata_outputs_and_existing_ids() {
    let original = sample();
    let before = serde_json::to_string(&original).unwrap();
    let edit = request(
        &before,
        vec![
            notebook::Change::Replace {
                index: 1,
                source: "print(2)\n".into(),
            },
            notebook::Change::Insert {
                index: 1,
                cell_type: notebook::Kind::Markdown,
                source: "## Inserted".into(),
            },
            notebook::Change::Delete { index: 2 },
        ],
    );
    let after = prepare(&before, &edit).unwrap();
    assert_eq!(
        after,
        prepare(&before, &edit).unwrap(),
        "checkpoint preparation must be deterministic"
    );
    let value = validation::parse(&after).unwrap();
    assert_eq!(value["metadata"], original["metadata"]);
    assert_eq!(value["extra"], original["extra"]);
    assert_eq!(value["cells"][0], original["cells"][0]);
    let mut expected = original["cells"][1].clone();
    expected["source"] = json!("print(2)\n");
    assert_eq!(value["cells"][2], expected);
    assert_eq!(value["cells"][1]["cell_type"], "markdown");
    assert_eq!(value["cells"][1]["id"].as_str().unwrap().len(), 64);
    assert_eq!(value["cells"].as_array().unwrap().len(), 3);
}

#[test]
fn appends_empty_code_cells_without_execution() {
    let before = sample().to_string();
    let after = prepare(
        &before,
        &request(
            &before,
            vec![notebook::Change::Insert {
                index: 3,
                cell_type: notebook::Kind::Code,
                source: "".into(),
            }],
        ),
    )
    .unwrap();
    let value = validation::parse(&after).unwrap();
    assert_eq!(value["cells"][3]["outputs"], json!([]));
    assert_eq!(value["cells"][3]["execution_count"], Value::Null);
}

#[test]
fn rejects_overlaps_and_stale_revisions() {
    let before = sample().to_string();
    let mut edit = request(
        &before,
        vec![
            notebook::Change::Replace {
                index: 0,
                source: "Changed".into(),
            },
            notebook::Change::Delete { index: 0 },
        ],
    );
    assert_eq!(
        prepare(&before, &edit).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    edit.edits.pop();
    edit.expected_revision = "a".repeat(64);
    assert_eq!(
        prepare(&before, &edit).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let mut unsupported = sample();
    unsupported["nbformat"] = json!(3);
    assert_eq!(
        validation::parse(&unsupported.to_string())
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    let mut duplicates = sample();
    duplicates["cells"][1]["id"] = json!("intro");
    assert_eq!(
        validation::parse(&duplicates.to_string()).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn bounds_cell_pages_without_changing_source() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let mut value = sample();
    value["cells"][1]["outputs"] = json!([{"output_type":"display_data","data":{"image/png":"a".repeat(notebook::MAX_PAGE_BYTES)},"metadata":{}}]);
    let before = value.to_string();
    std::fs::write(directory.path().join("sample.ipynb"), &before).unwrap();
    let page = read(
        &root,
        &notebook::Read {
            path: "sample.ipynb".into(),
            offset: 0,
            limit: 2,
        },
        &Control::new(CancellationToken::new()),
    )
    .unwrap();
    assert_eq!(page.next_offset, Some(2));
    assert_eq!(page.cells[0].value, value["cells"][0]);
    assert!(page.cells[1].truncated);
    assert!(page.cells[1].value.get("outputs").is_none());
    assert!(serde_json::to_vec(&page).unwrap().len() < notebook::MAX_PAGE_BYTES);
    assert_eq!(
        std::fs::read_to_string(directory.path().join("sample.ipynb")).unwrap(),
        before
    );
}

#[test]
fn reads_legacy_cells_without_rewriting() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let mut value = sample();
    value["nbformat_minor"] = json!(4);
    for cell in value["cells"].as_array_mut().unwrap() {
        cell.as_object_mut().unwrap().remove("id");
    }
    let before = value.to_string();
    let path = root.join("sample.ipynb");
    std::fs::write(&path, &before).unwrap();
    let page = read(
        &root,
        &notebook::Read {
            path: "sample.ipynb".into(),
            offset: 0,
            limit: 3,
        },
        &Control::new(CancellationToken::new()),
    )
    .unwrap();
    assert_eq!(page.nbformat_minor, 4);
    assert_eq!(page.cells.len(), 3);
    assert!(page.cells.iter().all(|cell| cell.value.get("id").is_none()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn pages_large_cell_requests_without_rewriting() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let mut value = sample();
    value["cells"] = Value::Array((0..notebook::MAX_CELLS + 1).map(|index| json!({"cell_type":"raw","id":format!("cell-{index}"),"source":"text","metadata":{}})).collect());
    let before = value.to_string();
    let path = root.join("sample.ipynb");
    std::fs::write(&path, &before).unwrap();
    let request = notebook::Read {
        path: "sample.ipynb".into(),
        offset: 0,
        limit: 1_000_000,
    };
    let first = read(&root, &request, &Control::new(CancellationToken::new())).unwrap();
    assert_eq!(first.cells.len(), notebook::MAX_CELLS);
    assert_eq!(first.next_offset, Some(notebook::MAX_CELLS));
    let next = read(
        &root,
        &notebook::Read {
            offset: first.next_offset.unwrap(),
            ..request
        },
        &Control::new(CancellationToken::new()),
    )
    .unwrap();
    assert_eq!(next.cells.len(), 1);
    assert_eq!(next.next_offset, None);
    assert_eq!(next.revision, first.revision);
    assert_eq!(std::fs::read_to_string(path).unwrap(), before);
}

#[test]
fn rejects_missing_current_ids_without_rewriting() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let mut value = sample();
    value["cells"][0].as_object_mut().unwrap().remove("id");
    let before = value.to_string();
    let path = root.join("sample.ipynb");
    std::fs::write(&path, &before).unwrap();
    let error = read(
        &root,
        &notebook::Read {
            path: "sample.ipynb".into(),
            offset: 0,
            limit: 3,
        },
        &Control::new(CancellationToken::new()),
    )
    .unwrap_err();
    assert_eq!(error.message, "notebook cell ID is missing");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn applies_large_batches_without_partial_changes() {
    let before = sample().to_string();
    let edits = (0..101)
        .map(|_| notebook::Change::Insert {
            index: 0,
            cell_type: notebook::Kind::Raw,
            source: "new".into(),
        })
        .collect();
    let input = notebook::Edit {
        path: "sample.ipynb".into(),
        expected_revision: blake3::hash(before.as_bytes()).to_hex().to_string(),
        edits,
    };
    let after = prepare(&before, &input).unwrap();
    let after: Value = serde_json::from_str(&after).unwrap();
    assert_eq!(
        after["cells"].as_array().unwrap().len(),
        sample()["cells"].as_array().unwrap().len() + 101
    );
    assert!(
        after["cells"].as_array().unwrap()[..101]
            .iter()
            .all(|cell| cell["source"] == "new")
    );
    let source = "x".repeat(512 * 1024 + 1);
    let input = notebook::Edit {
        edits: vec![notebook::Change::Replace {
            index: 0,
            source: source.clone(),
        }],
        ..input
    };
    let after = prepare(&before, &input).unwrap();
    let after: Value = serde_json::from_str(&after).unwrap();
    assert_eq!(after["cells"][0]["source"], source);
}
