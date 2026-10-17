use std::path::Path;

use git2::{BlameOptions, Blob, Oid, Repository};
use sailry_protocol::*;

use super::{Control, fault, history, open, valid_path};

pub(super) fn show(
    root: &Path,
    revision: &str,
    path: Option<&str>,
    offset: usize,
    limit: usize,
    control: &Control,
) -> Result<Output, Fault> {
    validate_page(offset, limit, MAX_GIT_SHOW_LINES)?;
    let repository = open(root)?;
    let commit = resolve(&repository, revision)?;
    let Some(path) = path else {
        if offset != 0 {
            return Err(Fault::invalid("commit patches do not accept a line offset"));
        }
        return history::commit(root, &commit.to_string(), control).map(Output::GitCommit);
    };
    let blob = blob(&repository, commit, path)?;
    let Some(text) = text(&blob) else {
        if offset != 0 {
            return Err(Fault::invalid(
                "binary Git files do not accept a line offset",
            ));
        }
        return Ok(Output::GitFile(GitFile {
            commit: commit.to_string(),
            path: path.to_owned(),
            offset,
            next: None,
            total_lines: 0,
            text: String::new(),
            binary: true,
            truncated: false,
            truncated_line: None,
        }));
    };
    let total_lines = count(text, control)?;
    check_offset(offset, total_lines)?;
    let mut output = String::new();
    let mut rendered = 0;
    let mut truncated_line = None;
    for (position, line) in text.lines().enumerate().skip(offset).take(limit) {
        control.check()?;
        let prefix = format!("{}: ", position + 1);
        let available = MAX_DIFF_BYTES.saturating_sub(output.len());
        if available <= prefix.len() + 1 {
            break;
        }
        let budget = available - prefix.len() - 1;
        if line.len() > budget && rendered > 0 {
            break;
        }
        let (line, partial) = bounded(line, budget);
        output.push_str(&prefix);
        output.push_str(line);
        output.push('\n');
        rendered += 1;
        if partial {
            truncated_line = Some(position + 1);
            break;
        }
    }
    let next = (offset + rendered < total_lines).then_some(offset + rendered);
    Ok(Output::GitFile(GitFile {
        commit: commit.to_string(),
        path: path.to_owned(),
        offset,
        next,
        total_lines,
        text: output,
        binary: false,
        truncated: next.is_some() || truncated_line.is_some(),
        truncated_line,
    }))
}

pub(super) fn blame(
    root: &Path,
    revision: Option<&str>,
    path: &str,
    offset: usize,
    limit: usize,
    control: &Control,
) -> Result<GitBlame, Fault> {
    validate_page(offset, limit, MAX_GIT_LINES)?;
    let repository = open(root)?;
    let commit = resolve(&repository, revision.unwrap_or("HEAD"))?;
    let blob = blob(&repository, commit, path)?;
    let text = text(&blob).ok_or_else(|| Fault::invalid("Git blame requires a UTF-8 text file"))?;
    let total_lines = count(text, control)?;
    check_offset(offset, total_lines)?;
    let mut entries = Vec::new();
    let mut bytes = 0;
    let mut partial = false;
    if offset < total_lines {
        let mut options = BlameOptions::new();
        options
            .newest_commit(commit)
            .min_line(offset + 1)
            .max_line((offset + limit).min(total_lines));
        control.check()?;
        let blame = repository
            .blame_file(Path::new(path), Some(&mut options))
            .map_err(fault)?;
        for (position, line) in text.lines().enumerate().skip(offset).take(limit) {
            control.check()?;
            let budget = MAX_DIFF_BYTES - bytes;
            if line.len() > budget && !entries.is_empty() {
                break;
            }
            let (line, truncated) = bounded(line, budget);
            let hunk = blame.get_line(position + 1).ok_or_else(|| {
                Fault::new(ErrorCode::Internal, "Git blame omitted a requested line")
            })?;
            let signature = hunk.final_signature().ok_or_else(|| {
                Fault::new(ErrorCode::Internal, "Git blame omitted the commit author")
            })?;
            let (author, author_partial) = bounded(signature.name().unwrap_or_default(), 256);
            let (email, email_partial) = bounded(signature.email().unwrap_or_default(), 256);
            entries.push(GitBlameLine {
                line: position + 1,
                original_line: hunk.orig_start_line() + position + 1 - hunk.final_start_line(),
                commit: hunk.final_commit_id().to_string(),
                author: author.to_owned(),
                email: email.to_owned(),
                timestamp: signature.when().seconds(),
                text: line.to_owned(),
                truncated: truncated || author_partial || email_partial,
            });
            bytes += line.len();
            partial |= truncated || author_partial || email_partial;
            if truncated || bytes == MAX_DIFF_BYTES {
                break;
            }
        }
    }
    let next = (offset + entries.len() < total_lines).then_some(offset + entries.len());
    Ok(GitBlame {
        commit: commit.to_string(),
        path: path.to_owned(),
        offset,
        next,
        total_lines,
        entries,
        truncated: partial || next.is_some(),
    })
}

fn resolve(repository: &Repository, revision: &str) -> Result<Oid, Fault> {
    if revision.trim().is_empty()
        || revision.len() > 1024
        || revision.starts_with('-')
        || revision.contains(':')
        || revision.chars().any(char::is_control)
    {
        return Err(Fault::invalid("invalid Git commit revision"));
    }
    repository
        .revparse_single(revision)
        .map_err(fault)?
        .peel_to_commit()
        .map(|commit| commit.id())
        .map_err(fault)
}

fn blob<'repo>(
    repository: &'repo Repository,
    commit: Oid,
    path: &str,
) -> Result<Blob<'repo>, Fault> {
    valid_path(path)?;
    let tree = repository
        .find_commit(commit)
        .map_err(fault)?
        .tree()
        .map_err(fault)?;
    let entry = tree.get_path(Path::new(path)).map_err(fault)?;
    if !matches!(entry.filemode(), 0o100644 | 0o100755) {
        return Err(Fault::invalid(
            "Git inspection requires a committed regular file",
        ));
    }
    let blob = repository.find_blob(entry.id()).map_err(fault)?;
    if blob.size() > MAX_GIT_FILE_BYTES {
        return Err(Fault::invalid(
            "Git file exceeds the 32 MiB inspection limit",
        ));
    }
    Ok(blob)
}

fn text<'blob>(blob: &'blob Blob<'_>) -> Option<&'blob str> {
    if blob.is_binary() {
        None
    } else {
        std::str::from_utf8(blob.content()).ok()
    }
}

fn validate_page(offset: usize, limit: usize, maximum: usize) -> Result<(), Fault> {
    if limit == 0 || limit > maximum {
        return Err(Fault::invalid(format!(
            "Git line page limit must be between 1 and {maximum}"
        )));
    }
    if offset.checked_add(limit).is_none() {
        return Err(Fault::invalid("Git line page offset exceeds the range"));
    }
    Ok(())
}

fn count(text: &str, control: &Control) -> Result<usize, Fault> {
    let mut total = 0;
    for _ in text.lines() {
        control.check()?;
        total += 1;
    }
    Ok(total)
}

fn check_offset(offset: usize, total: usize) -> Result<(), Fault> {
    if offset > total {
        return Err(Fault::invalid("Git line offset is beyond the file"));
    }
    Ok(())
}

fn bounded(text: &str, max: usize) -> (&str, bool) {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], end < text.len())
}
