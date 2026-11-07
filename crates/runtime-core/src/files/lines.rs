use super::{Control, io_error, path, read_bytes};
use sailry_protocol::{
    ErrorCode, Fault, FileLine, FileLines, MAX_EDIT_BYTES, MAX_FILE_BYTES, MAX_FILE_LINES,
};
use std::io::Read;

#[cfg(test)]
pub fn read<'a>(
    root: impl Into<super::Scope<'a>>,
    path: String,
    offset: usize,
    limit: usize,
    control: &Control,
) -> Result<FileLines, Fault> {
    read_version(root, path, offset, limit, None, control)
}

pub fn read_version<'a>(
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
    let mut file = path::file(scope, &path)?;
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
    let mut truncated = false;
    let mut skipping_line = false;
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
        for fragment in text.split_inclusive('\n') {
            if !collecting {
                if !truncated || next_offset.is_some() {
                    break;
                }
                if skipping_line {
                    if fragment.ends_with('\n') {
                        index += 1;
                        skipping_line = false;
                    }
                } else {
                    next_offset = Some(index);
                }
                continue;
            }
            if index >= offset {
                if lines.len() == page_limit {
                    next_offset = Some(index);
                    collecting = false;
                    continue;
                }
                if bytes + line.len() + fragment.len() > MAX_FILE_BYTES {
                    if lines.is_empty() {
                        let mut retained = MAX_FILE_BYTES - line.len();
                        while !fragment.is_char_boundary(retained) {
                            retained -= 1;
                        }
                        line.push_str(&fragment[..retained]);
                        lines.push(FileLine {
                            number: index + 1,
                            text: std::mem::take(&mut line),
                        });
                        truncated = true;
                        skipping_line = !fragment.ends_with('\n');
                        if !skipping_line {
                            index += 1;
                        }
                    } else {
                        next_offset = Some(index);
                        line.clear();
                    }
                    collecting = false;
                    continue;
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
        truncated,
    })
}

pub fn complete<'a>(
    root: impl Into<super::Scope<'a>>,
    relative: &str,
    control: &Control,
) -> Result<String, Fault> {
    bounded(root, relative, MAX_EDIT_BYTES, control)?
        .ok_or_else(|| Fault::invalid("text file exceeds the inspection limit"))
}

/// Read complete text through the ordinary confined file kernel, never a preview.
pub fn bounded<'a>(
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
    checked(path::file(scope, relative)?, limit, control)
}

/// [`bounded`] below a root handle that was already opened and confined, so a
/// caller reading many files does not reopen every root ancestor per file.
pub fn bounded_at(
    root: &cap_std::fs::Dir,
    access: super::Access,
    relative: &str,
    limit: usize,
    control: &Control,
) -> Result<Option<String>, Fault> {
    if limit == 0 || limit > MAX_EDIT_BYTES {
        return Err(Fault::invalid("text inspection limit is invalid"));
    }
    control.check()?;
    checked(path::file_at(root, access, relative)?, limit, control)
}

pub(super) fn checked(
    mut file: cap_std::fs::File,
    limit: usize,
    control: &Control,
) -> Result<Option<String>, Fault> {
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
