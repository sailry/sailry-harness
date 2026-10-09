use super::{Control, lines, save::Target};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault, FileEdit, FileWritten, MAX_EDIT_BYTES, MAX_FILE_BYTES, MAX_FILE_EDITS,
};
use std::{borrow::Cow, cmp::Reverse, collections::BinaryHeap, path::Path};
mod text;

pub(crate) fn changes<'a>(
    old_text: Option<&str>,
    new_text: Option<&str>,
    replace_all: bool,
    edits: Option<&'a [FileEdit]>,
) -> Result<Cow<'a, [FileEdit]>, Fault> {
    let changes = match (old_text, new_text, edits) {
        (Some(old_text), Some(new_text), None) => Cow::Owned(vec![FileEdit {
            old_text: old_text.into(),
            new_text: new_text.into(),
            replace_all,
        }]),
        (None, None, Some(edits)) if !replace_all => Cow::Borrowed(edits),
        _ => {
            return Err(Fault::invalid(
                "provide either old_text and new_text or an edits array",
            ));
        }
    };
    if changes.is_empty()
        || changes.len() > MAX_FILE_EDITS
        || changes.iter().any(|edit| {
            edit.old_text.is_empty()
                || edit.old_text.len() > MAX_FILE_BYTES
                || edit.new_text.len() > MAX_FILE_BYTES
                || edit.old_text.contains('\0')
                || edit.new_text.contains('\0')
        })
    {
        return Err(Fault::invalid("invalid file edit content or count"));
    }
    Ok(changes)
}

pub(crate) fn edit(
    root: &Path,
    path: &str,
    edits: &[FileEdit],
    expected: &str,
) -> Result<FileWritten, Fault> {
    let control = Control::new(CancellationToken::new());
    let before = lines::complete(root, path, &control)?;
    let text = prepare(&before, edits, expected)?;
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

pub(crate) fn prepare(before: &str, edits: &[FileEdit], expected: &str) -> Result<String, Fault> {
    changes(None, None, false, Some(edits))?;
    if before.len() > MAX_EDIT_BYTES
        || before.contains('\0')
        || blake3::Hash::from_hex(expected).is_err()
    {
        return Err(Fault::invalid("invalid file edit content or revision"));
    }
    if blake3::hash(before.as_bytes()).to_hex().as_str() != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "file changed before editing",
        ));
    }
    let control = Control::new(CancellationToken::new());
    let view = text::View::new(before);
    let patterns: Vec<_> = edits
        .iter()
        .map(|edit| text::Pattern::new(&edit.old_text))
        .collect();
    let mut matches = BinaryHeap::new();
    for (index, edit) in edits.iter().enumerate() {
        let pattern = &patterns[index];
        let start = pattern
            .find(&view, 0)
            .ok_or_else(|| Fault::invalid("old text was not found in the file"))?;
        if !edit.replace_all
            && pattern
                .find(&view, pattern.following(&view, start))
                .is_some()
        {
            return Err(Fault::invalid(
                "old text must occur exactly once in the file unless replace_all is true",
            ));
        }
        let range = pattern.range(&view, start);
        matches.push(Reverse((range.start, range.end, index, start)));
    }
    let mut after = String::with_capacity(before.len());
    let mut offset = 0;
    while let Some(Reverse((start, end, index, position))) = matches.pop() {
        control.check()?;
        if start < offset {
            return Err(Fault::invalid("file edit matches overlap"));
        }
        let edit = &edits[index];
        append(&mut after, &before[offset..start])?;
        append(
            &mut after,
            &patterns[index].replacement(before, start..end, &edit.new_text),
        )?;
        offset = end;
        if edit.replace_all {
            // Advance one character so overlapping occurrences are rejected instead of silently skipped.
            let pattern = &patterns[index];
            if let Some(next) = pattern.find(&view, pattern.following(&view, position)) {
                let range = pattern.range(&view, next);
                matches.push(Reverse((range.start, range.end, index, next)));
            }
        }
    }
    append(&mut after, &before[offset..])?;
    Ok(after)
}

fn append(after: &mut String, text: &str) -> Result<(), Fault> {
    if after
        .len()
        .checked_add(text.len())
        .is_none_or(|size| size > MAX_EDIT_BYTES)
    {
        return Err(Fault::invalid("edited file exceeds the text limit"));
    }
    after.push_str(text);
    Ok(())
}

#[cfg(test)]
mod tests;
