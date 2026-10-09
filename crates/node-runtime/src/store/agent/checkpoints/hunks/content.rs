use git2::Patch;
use sailry_protocol::{ErrorCode, Fault, MAX_DIFF_BYTES, MAX_EDIT_BYTES};
use std::path::Path;

pub(in crate::store) struct Change {
    pub id: String,
    pub old_start: u32,
    pub new_start: u32,
    pub text: String,
    pub before: String,
    pub after: String,
    pub additions: usize,
    pub deletions: usize,
}

pub(super) fn split(
    path: &str,
    before: &str,
    after: &str,
    segment: usize,
) -> Result<Vec<Change>, Fault> {
    let patch = Patch::from_buffers(
        before.as_bytes(),
        Some(Path::new(path)),
        after.as_bytes(),
        Some(Path::new(path)),
        None,
    )
    .map_err(fault)?;
    let mut changes = Vec::new();
    for index in 0..patch.num_hunks() {
        let (hunk, count) = patch.hunk(index).map_err(fault)?;
        let mut change = Change {
            id: String::new(),
            old_start: hunk.old_start(),
            new_start: hunk.new_start(),
            text: std::str::from_utf8(hunk.header())
                .map_err(fault)?
                .to_owned(),
            before: String::new(),
            after: String::new(),
            additions: 0,
            deletions: 0,
        };
        for line in 0..count {
            let line = patch.line_in_hunk(index, line).map_err(fault)?;
            let text = std::str::from_utf8(line.content()).map_err(fault)?;
            match line.origin() {
                ' ' => {
                    change.before.push_str(text);
                    change.after.push_str(text);
                }
                '+' => {
                    change.after.push_str(text);
                    change.additions += 1;
                }
                '-' => {
                    change.before.push_str(text);
                    change.deletions += 1;
                }
                _ => continue,
            }
            change.text.push(line.origin());
            change.text.push_str(text);
            if !text.ends_with('\n') {
                change.text.push_str("\n\\ No newline at end of file\n");
            }
        }
        if change.text.len() > MAX_DIFF_BYTES {
            return Err(Fault::invalid("hunk exceeds the review response limit"));
        }
        let mut identity = blake3::Hasher::new();
        for value in [path.as_bytes(), before.as_bytes(), after.as_bytes()] {
            identity.update(&(value.len() as u64).to_le_bytes());
            identity.update(value);
        }
        identity.update(&(segment as u64).to_le_bytes());
        identity.update(&(index as u64).to_le_bytes());
        change.id = identity.finalize().to_hex().to_string();
        changes.push(change);
    }
    Ok(changes)
}

/// Match exact context once; never use fuzzy patching to overwrite another edit.
pub(super) fn apply(current: &str, from: &str, to: &str) -> Result<String, Fault> {
    if current.len() > MAX_EDIT_BYTES || current.contains('\0') {
        return Err(Fault::invalid("hunk target exceeds the text limit"));
    }
    let start = if from.is_empty() {
        if !current.is_empty() {
            return Err(conflict());
        }
        0
    } else {
        let mut search = 0;
        let mut matched = None;
        while let Some(relative) = current[search..].find(from) {
            let start = search + relative;
            let end = start + from.len();
            let boundary = start == 0 || current.as_bytes()[start - 1] == b'\n';
            if boundary && (from.ends_with('\n') || end == current.len()) {
                if matched.is_some() {
                    return Err(conflict());
                }
                matched = Some(start);
            }
            search = start + current[start..].chars().next().unwrap().len_utf8();
        }
        matched.ok_or_else(conflict)?
    };
    let size = current
        .len()
        .checked_sub(from.len())
        .and_then(|size| size.checked_add(to.len()))
        .filter(|size| *size <= MAX_EDIT_BYTES)
        .ok_or_else(|| Fault::invalid("hunk result exceeds the text limit"))?;
    let mut result = String::with_capacity(size);
    result.push_str(&current[..start]);
    result.push_str(to);
    result.push_str(&current[start + from.len()..]);
    Ok(result)
}

fn conflict() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "hunk context changed or is ambiguous; inspect the file before retrying",
    )
}

fn fault(error: impl std::fmt::Display) -> Fault {
    Fault::invalid(format!("invalid review hunk: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_other_hunks_and_manual_edits() {
        let before = (0..30)
            .map(|line| format!("line {line}\n"))
            .collect::<String>();
        let after = before
            .replace("line 2\n", "changed 2\n")
            .replace("line 26\n", "changed 26\n");
        let changes = split("source.rs", &before, &after, 0).unwrap();
        assert_eq!(changes.len(), 2);
        let current = after.replace("line 15\n", "manual 15\n");
        let first = apply(&current, &changes[0].after, &changes[0].before).unwrap();
        assert_eq!(first, current.replace("changed 2\n", "line 2\n"));
        let second = apply(&first, &changes[1].after, &changes[1].before).unwrap();
        assert_eq!(second, before.replace("line 15\n", "manual 15\n"));
    }

    #[test]
    fn refuses_changed_and_ambiguous_context() {
        assert_eq!(
            apply("edited\n", "original\n", "replacement\n")
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            apply("same\nsame\n", "same\n", "replacement\n")
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            apply("aaaa", "aa", "b").unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            apply("manual", "", "new").unwrap_err().code,
            ErrorCode::RevisionConflict
        );
    }

    #[test]
    fn requires_line_boundaries_and_exact_eof() {
        assert_eq!(
            apply("manualchanged\n", "changed\n", "before\n")
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            apply("changed manual", "changed", "before")
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            apply("changed\n", "changed", "before").unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            apply("manualchanged\nchanged\n", "changed\n", "before\n").unwrap(),
            "manualchanged\nbefore\n"
        );
        assert_eq!(
            apply("\n\n\n", "\n\n", "replacement\n").unwrap_err().code,
            ErrorCode::RevisionConflict
        );
    }

    #[test]
    fn preserves_unicode_crlf_and_missing_newline() {
        for (before, after) in [
            ("first\r\nlast", "first\r\nchanged"),
            ("path 資料\nlast\n", "path 資料\nchanged\n"),
            ("", "new without newline"),
        ] {
            let changes = split("source.rs", before, after, 0).unwrap();
            assert_eq!(changes.len(), 1);
            assert_eq!(
                apply(before, &changes[0].before, &changes[0].after).unwrap(),
                after
            );
            assert_eq!(
                apply(after, &changes[0].after, &changes[0].before).unwrap(),
                before
            );
            assert_eq!(
                changes[0].id,
                split("source.rs", before, after, 0).unwrap()[0].id
            );
            assert_ne!(
                changes[0].id,
                split("different.rs", before, after, 0).unwrap()[0].id
            );
        }
    }
}
