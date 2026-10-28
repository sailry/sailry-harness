use super::{Control, io_error, open_regular, path, read_bytes};
use sailry_protocol::{
    ErrorCode, Fault, FileLine, FileLines, MAX_EDIT_BYTES, MAX_FILE_BYTES, MAX_FILE_LINES,
};
use std::io::Read;

#[cfg(test)]
pub(super) fn read<'a>(
    root: impl Into<super::Scope<'a>>,
    path: String,
    offset: usize,
    limit: usize,
    control: &Control,
) -> Result<FileLines, Fault> {
    read_version(root, path, offset, limit, None, control)
}

pub(super) fn read_version<'a>(
    root: impl Into<super::Scope<'a>>,
    path: String,
    offset: usize,
    limit: usize,
    expected: Option<&str>,
    control: &Control,
) -> Result<FileLines, Fault> {
    if limit == 0 {
        return Err(Fault::invalid("file line limit must be positive"));
    }
    let page_limit = limit.min(MAX_FILE_LINES);
    let scope = root.into();
    control.check()?;
    let parts = scope.components(&path, false)?;
    let (name, parents) = parts.split_last().unwrap();
    let directory = path::descend(path::root(scope.path())?, parents)?;
    let mut file = open_regular(&directory, name)?;
    let before = file.metadata().map_err(io_error)?;
    let mut hash = blake3::Hasher::new();
    let mut buffer = [0; 8192];
    let mut pending = 0;
    let mut total = 0u64;
    let mut lines = Vec::new();
    let mut bytes = 0usize;
    let mut next_offset = None;
    let mut index = 0;
    let mut line = String::new();
    let mut collecting = true;
    let mut oversized = false;
    loop {
        control.check()?;
        let count = file.read(&mut buffer[pending..]).map_err(io_error)?;
        if count == 0 {
            if pending != 0 {
                return Err(Fault::invalid("file is not UTF-8 text"));
            }
            break;
        }
        total = total
            .checked_add(count as u64)
            .filter(|total| *total <= before.len())
            .ok_or_else(|| {
                Fault::new(
                    ErrorCode::RevisionConflict,
                    "file changed during inspection",
                )
            })?;
        hash.update(&buffer[pending..pending + count]);
        let length = pending + count;
        let chunk = &buffer[..length];
        if chunk.contains(&0) {
            return Err(Fault::invalid("binary file cannot be displayed as text"));
        }
        let text = match std::str::from_utf8(chunk) {
            Ok(text) => text,
            Err(error) if error.error_len().is_none() => {
                std::str::from_utf8(&chunk[..error.valid_up_to()]).expect("validated UTF-8 prefix")
            }
            Err(_) => return Err(Fault::invalid("file is not UTF-8 text")),
        };
        let valid = text.len();
        // A full-content revision still covers every byte, while only the requested
        // page is retained. Keep at most one incomplete UTF-8 character between reads.
        if collecting {
            for fragment in text.split_inclusive('\n') {
                if index >= offset {
                    if lines.len() == page_limit
                        || bytes + line.len() + fragment.len() > MAX_FILE_BYTES
                    {
                        oversized = lines.is_empty();
                        next_offset = (!oversized).then_some(index);
                        line.clear();
                        collecting = false;
                        break;
                    }
                    line.push_str(fragment);
                }
                if fragment.ends_with('\n') {
                    if index >= offset {
                        bytes += line.len();
                        lines.push(FileLine {
                            number: index + 1,
                            text: std::mem::take(&mut line),
                        });
                    }
                    index += 1;
                }
            }
        }
        buffer.copy_within(valid..length, 0);
        pending = length - valid;
    }
    let after = file.metadata().map_err(io_error)?;
    if total != before.len()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "file changed during inspection",
        ));
    }
    let revision = hash.finalize().to_hex().to_string();
    if expected.is_some_and(|expected| expected != revision) {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "file changed before reading its indexed range",
        ));
    }
    if oversized {
        return Err(Fault::invalid("file line exceeds the page byte limit"));
    }
    if collecting && !line.is_empty() {
        lines.push(FileLine {
            number: index + 1,
            text: line,
        });
    }
    control.check()?;
    Ok(FileLines {
        path,
        lines,
        size: total,
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
    bounded(root, relative, MAX_EDIT_BYTES, control)?
        .ok_or_else(|| Fault::invalid("text file exceeds the inspection limit"))
}

/// Read complete text through the ordinary confined file kernel, never a preview.
pub(crate) fn bounded<'a>(
    root: impl Into<super::Scope<'a>>,
    relative: &str,
    limit: usize,
    control: &Control,
) -> Result<Option<String>, Fault> {
    if limit == 0 || limit > MAX_EDIT_BYTES {
        return Err(Fault::invalid("text inspection limit is invalid"));
    }
    let scope = root.into();
    control.check()?;
    let parts = scope.components(relative, false)?;
    let (name, parents) = parts.split_last().unwrap();
    let directory = path::descend(path::root(scope.path())?, parents)?;
    let mut file = open_regular(&directory, name)?;
    let before = file.metadata().map_err(io_error)?;
    if before.len() > limit as u64 {
        return Ok(None);
    }
    let bytes = read_bytes(&mut file, limit + 1, control)?;
    let after = file.metadata().map_err(io_error)?;
    if bytes.len() > limit
        || bytes.len() as u64 != before.len()
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
    Ok(Some(text))
}

#[cfg(test)]
mod tests;
