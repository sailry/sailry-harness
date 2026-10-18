use super::{Control, io_error, open_regular, path, read_bytes};
use sailry_protocol::{
    ErrorCode, Fault, FileLine, FileLines, MAX_EDIT_BYTES, MAX_FILE_BYTES, MAX_FILE_LINES,
};

pub(super) fn read<'a>(
    root: impl Into<super::Scope<'a>>,
    path: String,
    offset: usize,
    limit: usize,
    control: &Control,
) -> Result<FileLines, Fault> {
    if limit == 0 {
        return Err(Fault::invalid("file line limit must be positive"));
    }
    let page_limit = limit.min(MAX_FILE_LINES);
    let text = complete(root, &path, control)?;
    let size = text.len() as u64;
    let revision = blake3::hash(text.as_bytes()).to_hex().to_string();
    let mut lines = Vec::new();
    let mut bytes = 0usize;
    let mut next_offset = None;
    for (index, line) in text.split_inclusive('\n').enumerate() {
        control.check()?;
        if index < offset {
            continue;
        }
        if lines.len() == page_limit || bytes + line.len() > MAX_FILE_BYTES {
            if lines.is_empty() {
                return Err(Fault::invalid("file line exceeds the page byte limit"));
            }
            next_offset = Some(index);
            break;
        }
        bytes += line.len();
        lines.push(FileLine {
            number: index + 1,
            text: line.into(),
        });
    }
    control.check()?;
    Ok(FileLines {
        path,
        lines,
        size,
        revision,
        offset,
        next_offset,
    })
}

pub(super) fn complete<'a>(
    root: impl Into<super::Scope<'a>>,
    relative: &str,
    control: &Control,
) -> Result<String, Fault> {
    let scope = root.into();
    control.check()?;
    let parts = scope.components(relative, false)?;
    let (name, parents) = parts.split_last().unwrap();
    let directory = path::descend(path::root(scope.path())?, parents)?;
    let mut file = open_regular(&directory, name)?;
    let before = file.metadata().map_err(io_error)?;
    if before.len() > MAX_EDIT_BYTES as u64 {
        return Err(Fault::invalid("text file exceeds the inspection limit"));
    }
    let bytes = read_bytes(&mut file, MAX_EDIT_BYTES + 1, control)?;
    let after = file.metadata().map_err(io_error)?;
    if bytes.len() as u64 != before.len()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "file changed during inspection",
        ));
    }
    let text = String::from_utf8(bytes).map_err(|_| Fault::invalid("file is not UTF-8 text"))?;
    if text.contains('\0') {
        return Err(Fault::invalid("binary file cannot be displayed as text"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests;
