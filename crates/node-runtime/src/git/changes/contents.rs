use std::{io::Read, path::Path};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::OpenOptions;
use git2::{DiffOptions, ObjectType, Oid, Patch, Repository};
use sailry_protocol::*;

use super::{
    Page, encoded,
    inventory::{Candidate, Resource, Stamp},
    stale,
};
use crate::git::{Control, fault, io_error, path};

enum Bytes {
    Ready(Vec<u8>),
    Skipped(&'static str),
}

pub(super) fn read(
    root: &Path,
    repository: &Repository,
    snapshot: &GitChangeSnapshot,
    candidate: &Candidate,
    page: Page<'_>,
    control: &Control,
) -> Result<GitChange, Fault> {
    let Page {
        expected_revision,
        offset,
        limit,
    } = page;
    let (old, new, revision) = capture(root, repository, candidate, control)?;
    let skip = match (&old, &new) {
        (Bytes::Skipped(reason), _) | (_, Bytes::Skipped(reason)) => Some(*reason),
        _ => None,
    };
    if expected_revision.is_some_and(|expected| expected != revision) {
        return Err(stale());
    }
    let mut result = GitChange {
        snapshot: snapshot.clone(),
        path: candidate.entry.path.clone(),
        old_path: candidate.entry.old_path.clone(),
        metadata_revision: candidate.entry.metadata_revision.clone(),
        revision,
        offset,
        next: None,
        lines: Vec::new(),
        binary: false,
        skip_reason: skip.map(str::to_owned),
        first_line: None,
    };
    let (Bytes::Ready(old), Bytes::Ready(new)) = (old, new) else {
        if offset != 0 {
            return Err(Fault::invalid("skipped Git changes do not have line pages"));
        }
        return Ok(result);
    };
    if old.contains(&0) || new.contains(&0) {
        result.binary = true;
        result.skip_reason = Some("binary".into());
    } else if std::str::from_utf8(&old).is_err() || std::str::from_utf8(&new).is_err() {
        result.skip_reason = Some("non_utf8".into());
    }
    if result.skip_reason.is_some() {
        if offset != 0 {
            return Err(Fault::invalid("skipped Git changes do not have line pages"));
        }
        return Ok(result);
    }
    let inspected = if candidate.new.is_none() { &old } else { &new };
    result.first_line = std::str::from_utf8(inspected)
        .expect("validated UTF-8")
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| prefix(line.trim_start(), 256).to_owned());
    control.check()?;
    let mut options = DiffOptions::new();
    options.context_lines(3).force_text(true);
    // Diff exactly the retained bytes whose strong revision was computed above.
    let patch = Patch::from_buffers(
        &old,
        Some(Path::new(&result.path)),
        &new,
        Some(Path::new(&result.path)),
        Some(&mut options),
    )
    .map_err(fault)?;
    let mut position = 0;
    let mut full = false;
    for hunk in 0..patch.num_hunks() {
        for index in 0..patch.num_lines_in_hunk(hunk).map_err(fault)? {
            control.check()?;
            let line = patch.line_in_hunk(hunk, index).map_err(fault)?;
            let kind = match line.origin() {
                '+' => GitChangeLineKind::Added,
                '-' => GitChangeLineKind::Deleted,
                ' ' => GitChangeLineKind::Context,
                _ => continue,
            };
            let text = std::str::from_utf8(line.content())
                .map_err(|_| Fault::new(ErrorCode::Internal, "Git diff returned invalid UTF-8"))?;
            let text = text
                .strip_suffix('\n')
                .map(|text| text.strip_suffix('\r').unwrap_or(text))
                .unwrap_or(text);
            let mapped = GitChangeLine {
                kind,
                old_line: line.old_lineno(),
                new_line: line.new_lineno(),
                text: text.to_owned(),
            };
            // A single oversized line makes the whole file explicitly unreviewable;
            // never pretend a truncated source line is a complete anchor.
            let mut single = result.clone();
            single.lines = vec![mapped.clone()];
            single.next = Some(position + 1);
            if encoded(&Output::GitChange(single))? > MAX_DIFF_BYTES {
                result.lines.clear();
                result.next = None;
                result.skip_reason = Some("line_too_large".into());
                return Ok(result);
            }
            if position >= offset && !full {
                result.lines.push(mapped);
                result.next = Some(position + 1);
                if result.lines.len() > limit
                    || encoded(&Output::GitChange(result.clone()))? > MAX_DIFF_BYTES
                {
                    result.lines.pop();
                    result.next = Some(position);
                    full = true;
                }
            }
            position += 1;
        }
    }
    if offset > position {
        return Err(Fault::invalid("Git change offset is beyond the diff"));
    }
    if !full {
        result.next = None;
    }
    // Binary/empty and last-page responses also include the complete envelope bound.
    if encoded(&Output::GitChange(result.clone()))? > MAX_DIFF_BYTES {
        return Err(Fault::invalid(
            "Git change metadata exceeds the page size limit",
        ));
    }
    Ok(result)
}

fn capture(
    root: &Path,
    repository: &Repository,
    candidate: &Candidate,
    control: &Control,
) -> Result<(Bytes, Bytes, String), Fault> {
    let old = bytes(
        root,
        repository,
        &candidate.entry.path,
        candidate.old.as_ref(),
        control,
    )?;
    let new = bytes(
        root,
        repository,
        &candidate.entry.path,
        candidate.new.as_ref(),
        control,
    )?;
    let mut revision = blake3::Hasher::new();
    revision.update(candidate.entry.metadata_revision.as_bytes());
    for content in [&old, &new] {
        match content {
            Bytes::Ready(bytes) => {
                revision.update(&(bytes.len() as u64).to_le_bytes());
                revision.update(bytes);
            }
            Bytes::Skipped(reason) => {
                revision.update(reason.as_bytes());
            }
        }
    }
    let revision = revision.finalize().to_hex().to_string();
    Ok((old, new, revision))
}

pub(super) fn context(
    root: &Path,
    repository: &Repository,
    snapshot: &GitChangeSnapshot,
    candidate: &Candidate,
    side: GitContextSide,
    page: Page<'_>,
    control: &Control,
) -> Result<GitContext, Fault> {
    let (old, new, revision) = capture(root, repository, candidate, control)?;
    if page.expected_revision != Some(revision.as_str()) {
        return Err(stale());
    }
    let mut result = GitContext {
        snapshot: snapshot.clone(),
        path: candidate.entry.path.clone(),
        old_path: candidate.entry.old_path.clone(),
        metadata_revision: candidate.entry.metadata_revision.clone(),
        revision,
        side,
        offset: page.offset,
        next: None,
        text: String::new(),
        binary: false,
        skip_reason: None,
    };
    for content in [&old, &new] {
        match content {
            Bytes::Skipped(reason) => result.skip_reason = Some((*reason).into()),
            Bytes::Ready(bytes) if bytes.contains(&0) => {
                result.binary = true;
                result.skip_reason = Some("binary".into());
            }
            Bytes::Ready(bytes) if std::str::from_utf8(bytes).is_err() => {
                result.skip_reason = Some("non_utf8".into());
            }
            _ => {}
        }
    }
    if result.skip_reason.is_some() {
        if page.offset != 0 {
            return Err(Fault::invalid(
                "skipped Git context does not have line pages",
            ));
        }
        return Ok(result);
    }
    let Bytes::Ready(bytes) = (match side {
        GitContextSide::Old => old,
        GitContextSide::New => new,
    }) else {
        unreachable!("skipped sources returned above")
    };
    let text = std::str::from_utf8(&bytes).expect("validated UTF-8");
    let mut position = 0;
    let mut full = false;
    for line in text.split_inclusive('\n') {
        control.check()?;
        if position >= page.offset && !full {
            let mut single = result.clone();
            single.text = line.into();
            single.next = Some(position + 1);
            if encoded(&Output::GitContext(single))? > MAX_DIFF_BYTES {
                result.text.clear();
                result.next = None;
                result.skip_reason = Some("line_too_large".into());
                return Ok(result);
            }
            let previous = result.text.len();
            result.text.push_str(line);
            result.next = Some(position + 1);
            if position - page.offset >= page.limit.min(MAX_GIT_LINES)
                || encoded(&Output::GitContext(result.clone()))? > MAX_DIFF_BYTES
            {
                result.text.truncate(previous);
                result.next = Some(position);
                full = true;
            }
        }
        position += 1;
    }
    if page.offset > position {
        return Err(Fault::invalid("Git context offset is beyond the source"));
    }
    if !full {
        result.next = None;
    }
    if encoded(&Output::GitContext(result.clone()))? > MAX_DIFF_BYTES {
        return Err(Fault::invalid(
            "Git context metadata exceeds the page size limit",
        ));
    }
    Ok(result)
}

fn bytes(
    root: &Path,
    repository: &Repository,
    name: &str,
    resource: Option<&Resource>,
    control: &Control,
) -> Result<Bytes, Fault> {
    let Some(resource) = resource else {
        return Ok(Bytes::Ready(Vec::new()));
    };
    if !matches!(resource.mode(), 0o100644 | 0o100755) {
        return Ok(Bytes::Skipped("unsupported_mode"));
    }
    control.check()?;
    match resource {
        Resource::Blob { oid, .. } => {
            let oid = Oid::from_str(oid).map_err(fault)?;
            let (size, kind) = repository
                .odb()
                .map_err(fault)?
                .read_header(oid)
                .map_err(fault)?;
            if kind != ObjectType::Blob {
                return Ok(Bytes::Skipped("unsupported_mode"));
            }
            if size > MAX_GIT_FILE_BYTES {
                return Ok(Bytes::Skipped("oversized"));
            }
            Ok(Bytes::Ready(
                repository.find_blob(oid).map_err(fault)?.content().to_vec(),
            ))
        }
        Resource::Working(expected) => {
            if expected.size > MAX_GIT_FILE_BYTES as u64 {
                return Ok(Bytes::Skipped("oversized"));
            }
            let parts = path::components(name, false)?;
            let directory = path::descend(path::root(root)?, &parts[..parts.len() - 1])?;
            let mut options = OpenOptions::new();
            options.read(true).follow(FollowSymlinks::No);
            #[cfg(unix)]
            cap_std::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
            let mut file = directory
                .open_with(parts[parts.len() - 1], &options)
                .map_err(io_error)?;
            if Stamp::of(&file.metadata().map_err(io_error)?) != *expected {
                return Err(stale());
            }
            let mut bytes = Vec::new();
            (&mut file)
                .take(MAX_GIT_FILE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(io_error)?;
            control.check()?;
            if Stamp::of(&file.metadata().map_err(io_error)?) != *expected {
                return Err(stale());
            }
            if bytes.len() > MAX_GIT_FILE_BYTES {
                return Ok(Bytes::Skipped("oversized"));
            }
            Ok(Bytes::Ready(bytes))
        }
    }
}

fn prefix(text: &str, limit: usize) -> &str {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
