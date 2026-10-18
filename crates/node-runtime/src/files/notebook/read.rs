use super::*;

pub(in crate::files) fn read<'a>(
    root: impl Into<crate::files::Scope<'a>>,
    input: &notebook::Read,
    control: &Control,
) -> Result<notebook::Page, Fault> {
    if input.limit == 0 {
        return Err(Fault::invalid("notebook cell limit must be positive"));
    }
    let page_limit = input.limit.min(notebook::MAX_CELLS);
    let text = lines::complete(root, &input.path, control)?;
    let value = validation::parse(&text)?;
    let original = value["cells"].as_array().expect("validated notebook cells");
    let mut metadata = value["metadata"].clone();
    let metadata_truncated = size(&metadata)? > sailry_protocol::MAX_FILE_BYTES;
    if metadata_truncated {
        metadata = json!({});
    }
    let mut bytes = size(&metadata)? + 4096;
    let mut cells = Vec::new();
    let mut next_offset = None;
    for (index, value) in original.iter().enumerate().skip(input.offset) {
        control.check()?;
        let mut cell = notebook::Cell {
            index,
            value: value.clone(),
            truncated: false,
        };
        if size(&cell.value)? > notebook::MAX_PAGE_BYTES / 2 {
            let mut source = validation::text_source(&value["source"])?;
            let mut end = source.len().min(sailry_protocol::MAX_FILE_BYTES);
            while !source.is_char_boundary(end) {
                end -= 1;
            }
            source.truncate(end);
            cell.value = json!({"cell_type":value["cell_type"],"id":value.get("id"),"source":source,"metadata":{}});
            cell.truncated = true;
        }
        let length = serde_json::to_vec(&cell)
            .map_err(|_| Fault::invalid("invalid notebook cell JSON"))?
            .len();
        if cells.len() == page_limit || bytes + length > notebook::MAX_PAGE_BYTES {
            next_offset = Some(index);
            break;
        }
        bytes += length;
        cells.push(cell);
    }
    control.check()?;
    Ok(notebook::Page {
        path: input.path.clone(),
        revision: blake3::hash(text.as_bytes()).to_hex().to_string(),
        nbformat: 4,
        nbformat_minor: value["nbformat_minor"].as_u64().unwrap() as u32,
        metadata,
        metadata_truncated,
        total: original.len(),
        cells,
        next_offset,
    })
}

fn size(value: &Value) -> Result<usize, Fault> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|_| Fault::invalid("invalid notebook JSON"))
}
