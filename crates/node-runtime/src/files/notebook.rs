//! Notebook edits share confined reads and the ordinary revision-checked save path.
use super::{Control, lines, save::Target};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, FileWritten, MAX_EDIT_BYTES, notebook};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

mod creation;
mod read;
#[cfg(test)]
mod tests;
mod validation;
pub(crate) use creation::prepare as prepare_creation;
pub(super) use read::read;

pub(crate) fn edit<'a>(
    root: impl Into<super::Scope<'a>>,
    input: &notebook::Edit,
) -> Result<FileWritten, Fault> {
    let root = root.into();
    let control = Control::new(CancellationToken::new());
    let before = lines::complete(root, &input.path, &control)?;
    let after = prepare(&before, input)?;
    control.check()?;
    let target = Target::prepare(
        root,
        &input.path,
        Some(&input.expected_revision),
        MAX_EDIT_BYTES as u64,
        Some(&control.cancelled),
    )?;
    let revision = blake3::hash(after.as_bytes()).to_hex().to_string();
    target.write(
        after.as_bytes(),
        after.len() as u64,
        &revision,
        #[cfg(test)]
        &|_| {},
    )
}

pub(crate) fn prepare(before: &str, input: &notebook::Edit) -> Result<String, Fault> {
    if blake3::Hash::from_hex(&input.expected_revision).is_err() {
        return Err(Fault::invalid("invalid notebook revision"));
    }
    if blake3::hash(before.as_bytes()).to_hex().as_str() != input.expected_revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "notebook changed before editing",
        ));
    }
    if input.edits.is_empty() || input.edits.len() > notebook::MAX_CELLS {
        return Err(Fault::invalid("notebook edit count is out of range"));
    }
    let mut value = validation::parse(before)?;
    let cells = value["cells"]
        .as_array_mut()
        .expect("validated notebook cells");
    let mut changes = BTreeMap::new();
    let mut insertions: BTreeMap<usize, Vec<Value>> = BTreeMap::new();
    let mut ids: BTreeSet<String> = cells
        .iter()
        .filter_map(|cell| cell["id"].as_str().map(str::to_owned))
        .collect();
    for (ordinal, change) in input.edits.iter().enumerate() {
        match change {
            notebook::Change::Replace { index, source } => {
                validation::source(source)?;
                if *index >= cells.len() || changes.insert(*index, Some(source)).is_some() {
                    return Err(Fault::invalid(
                        "notebook edit indices overlap or are out of range",
                    ));
                }
            }
            notebook::Change::Delete { index } => {
                if *index >= cells.len() || changes.insert(*index, None).is_some() {
                    return Err(Fault::invalid(
                        "notebook edit indices overlap or are out of range",
                    ));
                }
            }
            notebook::Change::Insert {
                index,
                cell_type,
                source,
            } => {
                validation::source(source)?;
                if *index > cells.len() {
                    return Err(Fault::invalid("notebook insertion index is out of range"));
                }
                // A deterministic ID keeps the approved checkpoint and durable write identical.
                let mut nonce = 0usize;
                let id = loop {
                    let seed = format!("{}:{ordinal}:{nonce}", input.expected_revision);
                    let id = blake3::hash(seed.as_bytes()).to_hex().to_string();
                    if ids.insert(id.clone()) {
                        break id;
                    }
                    nonce += 1;
                };
                let mut cell = json!({"id":id,"cell_type":cell_type,"source":source,"metadata":{}});
                if *cell_type == notebook::Kind::Code {
                    cell["outputs"] = json!([]);
                    cell["execution_count"] = Value::Null;
                }
                insertions.entry(*index).or_default().push(cell);
            }
        }
    }
    let count = cells.len();
    let original = std::mem::take(cells);
    for (index, mut cell) in original.into_iter().enumerate() {
        if let Some(inserted) = insertions.remove(&index) {
            cells.extend(inserted);
        }
        match changes.remove(&index) {
            Some(Some(source)) => {
                cell["source"] = Value::String(source.clone());
                cells.push(cell);
            }
            Some(None) => {}
            None => cells.push(cell),
        }
    }
    if let Some(inserted) = insertions.remove(&count) {
        cells.extend(inserted);
    }
    if cells.len() > validation::MAX_COUNT {
        return Err(Fault::invalid(
            "edited notebook cell count exceeds the limit",
        ));
    }
    let mut after = serde_json::to_string_pretty(&value)
        .map_err(|_| Fault::invalid("invalid notebook JSON"))?;
    after.push('\n');
    if after.len() > MAX_EDIT_BYTES {
        return Err(Fault::invalid("edited notebook exceeds the text limit"));
    }
    Ok(after)
}
