use super::*;

pub(super) fn parse(text: &str) -> Result<Value, Fault> {
    parse_with_ids(text, true)
}

pub(super) fn draft(text: &str) -> Result<Value, Fault> {
    parse_with_ids(text, false)
}

fn parse_with_ids(text: &str, require_ids: bool) -> Result<Value, Fault> {
    if text.len() > MAX_EDIT_BYTES || text.contains('\0') {
        return Err(Fault::invalid("notebook exceeds the text limit"));
    }
    let value: Value =
        serde_json::from_str(text).map_err(|_| Fault::invalid("notebook is not valid JSON"))?;
    if value["nbformat"].as_u64() != Some(4)
        || value["nbformat_minor"]
            .as_u64()
            .is_none_or(|minor| minor > 5)
        || !value["metadata"].is_object()
    {
        return Err(Fault::invalid("notebook must use nbformat 4"));
    }
    let cells = value["cells"]
        .as_array()
        .ok_or_else(|| Fault::invalid("notebook cells must be an array"))?;
    let mut ids = BTreeSet::new();
    for cell in cells {
        if !cell.is_object()
            || !cell["metadata"].is_object()
            || !matches!(
                cell["cell_type"].as_str(),
                Some("code" | "markdown" | "raw")
            )
        {
            return Err(Fault::invalid("invalid notebook cell"));
        }
        text_source(&cell["source"])?;
        if cell["cell_type"] == "code"
            && (!cell["outputs"].is_array()
                || !cell
                    .get("execution_count")
                    .is_some_and(|count| count.is_null() || count.as_u64().is_some()))
        {
            return Err(Fault::invalid(
                "invalid notebook code outputs or execution count",
            ));
        }
        if let Some(id) = cell.get("id") {
            let id = id
                .as_str()
                .ok_or_else(|| Fault::invalid("invalid notebook cell ID"))?;
            if id.is_empty()
                || id.len() > 64
                || !id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                || !ids.insert(id)
            {
                return Err(Fault::invalid("invalid or duplicate notebook cell ID"));
            }
        } else if require_ids && value["nbformat_minor"].as_u64() == Some(5) {
            return Err(Fault::invalid("notebook cell ID is missing"));
        }
    }
    Ok(value)
}

pub(super) fn source(source: &str) -> Result<(), Fault> {
    if source.contains('\0') {
        Err(Fault::invalid("notebook cell source contains NUL"))
    } else {
        Ok(())
    }
}

pub(super) fn text_source(value: &Value) -> Result<String, Fault> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Array(lines) if lines.iter().all(Value::is_string) => {
            Ok(lines.iter().map(|line| line.as_str().unwrap()).collect())
        }
        _ => Err(Fault::invalid(
            "notebook source must be text or an array of text",
        )),
    }
}
