//! Prepare generated notebook text before its approval and checkpoint capture.
use super::*;
use std::borrow::Cow;

pub(crate) fn prepare<'a>(path: &str, text: &'a str) -> Result<Cow<'a, str>, Fault> {
    if !Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ipynb"))
    {
        return Ok(Cow::Borrowed(text));
    }
    let mut value = validation::draft(text)?;
    if value["nbformat_minor"].as_u64() != Some(5) {
        return Ok(Cow::Borrowed(text));
    }
    let cells = value["cells"]
        .as_array_mut()
        .expect("validated notebook cells");
    let mut ids: BTreeSet<String> = cells
        .iter()
        .filter_map(|cell| cell["id"].as_str().map(str::to_owned))
        .collect();
    let revision = blake3::hash(text.as_bytes()).to_hex();
    let mut changed = false;
    for (index, cell) in cells.iter_mut().enumerate() {
        if cell.get("id").is_some() {
            continue;
        }
        let mut nonce = 0usize;
        let id = loop {
            let seed = format!("{revision}:{index}:{nonce}");
            let id = blake3::hash(seed.as_bytes()).to_hex().to_string();
            if ids.insert(id.clone()) {
                break id;
            }
            nonce += 1;
        };
        cell["id"] = id.into();
        changed = true;
    }
    if !changed {
        return Ok(Cow::Borrowed(text));
    }
    let mut result = serde_json::to_string_pretty(&value)
        .map_err(|_| Fault::invalid("invalid notebook JSON"))?;
    result.push('\n');
    validation::parse(&result)?;
    Ok(Cow::Owned(result))
}

#[cfg(test)]
mod tests;
