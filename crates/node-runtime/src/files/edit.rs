use super::{Control, lines, save::Target};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, FileWritten, MAX_EDIT_BYTES, MAX_FILE_BYTES};
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub(crate) fn edit(
    root: &Path,
    path: &str,
    old_text: &str,
    new_text: &str,
    expected: &str,
) -> Result<FileWritten, Fault> {
    let control = Control {
        cancelled: CancellationToken::new(),
        deadline: Instant::now() + Duration::from_secs(5),
    };
    let before = lines::complete(root, path, &control)?;
    let text = prepare(&before, old_text, new_text, expected)?;
    control.check()?;
    let target = Target::prepare(
        root,
        path,
        Some(expected),
        MAX_EDIT_BYTES as u64,
        Some(&control.cancelled),
    )?;
    control.check()?;
    let revision = blake3::hash(text.as_bytes()).to_hex().to_string();
    target.write(
        text.as_bytes(),
        text.len() as u64,
        &revision,
        #[cfg(test)]
        &|_| {},
    )
}

pub(crate) fn prepare(
    before: &str,
    old_text: &str,
    new_text: &str,
    expected: &str,
) -> Result<String, Fault> {
    if old_text.is_empty()
        || old_text.len() > MAX_FILE_BYTES
        || new_text.len() > MAX_FILE_BYTES
        || old_text.contains('\0')
        || new_text.contains('\0')
        || before.len() > MAX_EDIT_BYTES
        || before.contains('\0')
        || expected.len() != 64
        || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Fault::invalid("invalid file edit content or revision"));
    }
    if blake3::hash(before.as_bytes()).to_hex().as_str() != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "file changed before editing",
        ));
    }
    let start = before
        .find(old_text)
        .ok_or_else(|| Fault::invalid("old text was not found in the file"))?;
    let following = start + before[start..].chars().next().unwrap().len_utf8();
    if before[following..].contains(old_text) {
        return Err(Fault::invalid(
            "old text must occur exactly once in the file",
        ));
    }
    let size = before
        .len()
        .checked_sub(old_text.len())
        .and_then(|size| size.checked_add(new_text.len()))
        .filter(|size| *size <= MAX_EDIT_BYTES)
        .ok_or_else(|| Fault::invalid("edited file exceeds the text limit"))?;
    let mut after = String::with_capacity(size);
    after.push_str(&before[..start]);
    after.push_str(new_text);
    after.push_str(&before[start + old_text.len()..]);
    Ok(after)
}

#[cfg(test)]
mod tests;
