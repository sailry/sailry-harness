//! Literal ignore rules use the existing revision-checked file save boundary.
use super::{fault, io_error, path, valid_path};
use sailry_protocol::{ErrorCode, Fault, MAX_FILE_BYTES, MAX_GIT_ENTRIES};
use std::{io::Read, path::Path};

pub(super) fn add(
    root: &Path,
    repository: &git2::Repository,
    paths: &[String],
    directories: bool,
) -> Result<(), Fault> {
    if paths.is_empty() || paths.len() > MAX_GIT_ENTRIES {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid ignore targets",
        ));
    }
    for value in paths {
        valid_path(value)?;
        let parts = path::components(value, false)?;
        let dir = path::descend(path::root(root)?, &parts[..parts.len() - 1])?;
        let metadata = dir
            .symlink_metadata(parts[parts.len() - 1])
            .map_err(io_error)?;
        if directories {
            if !metadata.is_dir() || metadata.is_symlink() {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "ignore target is not a directory",
                ));
            }
        } else {
            if !metadata.is_file() && !metadata.is_symlink() {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "ignore target is not a file",
                ));
            }
            let status = repository.status_file(Path::new(value)).map_err(fault)?;
            if !status.is_wt_new() && !status.is_ignored() {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "tracked files cannot be ignored",
                ));
            }
        }
    }
    let dir = path::root(root)?;
    let (mut text, revision) = match crate::files::open_regular(&dir, ".gitignore") {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_FILE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(io_error)?;
            if bytes.len() > MAX_FILE_BYTES {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "ignore file exceeds the edit limit",
                ));
            }
            let revision = blake3::hash(&bytes).to_hex().to_string();
            let text = String::from_utf8(bytes).map_err(|_| {
                Fault::new(ErrorCode::InvalidRequest, "ignore file is not UTF-8 text")
            })?;
            (text, Some(revision))
        }
        Err(error) if error.code == ErrorCode::NotFound => (String::new(), None),
        Err(error) => return Err(error),
    };
    let ending = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let original = text.len();
    let mut appended = std::collections::HashSet::new();
    for value in paths {
        let rule = pattern(value, directories);
        if !appended.insert(rule.clone()) {
            continue;
        }
        // Skip effective duplicates, but allow a later explicit rule to override a negation.
        if text.lines().any(|line| line == rule)
            && repository
                .status_should_ignore(Path::new(value))
                .map_err(fault)?
        {
            continue;
        }
        if !text.is_empty() && !text.ends_with('\n') {
            text.push_str(ending);
        }
        text.push_str(&rule);
        text.push_str(ending);
    }
    if text.len() != original {
        crate::files::save(root, ".gitignore", &text, revision.as_deref())?;
    }
    Ok(())
}

fn pattern(value: &str, directory: bool) -> String {
    let mut pattern = String::from("/");
    for character in value.chars() {
        if matches!(character, '*' | '?' | '[' | ']' | '\\' | ' ' | '#' | '!') {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    if directory {
        pattern.push('/');
    }
    pattern
}
