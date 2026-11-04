//! Markdown paths and file URLs resolve against the originating worktree.
use sailry_protocol::{Worktree, WorktreeId};
use std::path::PathBuf;

#[derive(Debug, PartialEq)]
pub(crate) enum Target {
    Workspace { path: String, line: Option<usize> },
    External(PathBuf),
}

/// Absolute links may name another registered root on the same execution Node.
pub(crate) fn locate(
    link: &str,
    origin: WorktreeId,
    roots: &[Worktree],
) -> Option<(WorktreeId, Target)> {
    let current = roots.iter().find(|entry| entry.id == origin)?;
    let target = resolve(link, &current.path)?;
    let (path, _) = parts(link)?;
    let absolute = path.starts_with(['/', '\\'])
        || path.as_bytes().get(1) == Some(&b':')
        || url::Url::parse(path).is_ok_and(|url| url.scheme() == "file");
    if absolute
        && let Some((entry, target)) = roots
            .iter()
            .filter_map(|entry| match resolve(link, &entry.path)? {
                target @ Target::Workspace { .. } => Some((entry, target)),
                Target::External(_) => None,
            })
            .max_by_key(|(entry, _)| entry.path.len())
    {
        return Some((entry.id, target));
    }
    Some((origin, target))
}

/// Resolve file URLs and Markdown paths against the captured execution directory.
pub(crate) fn resolve(link: &str, root: &str) -> Option<Target> {
    let (target, line) = parts(link)?;
    let root = root.replace('\\', "/");
    let mut base = url::Url::parse("file:///").ok()?;
    base.set_path(&format!("{}/", root.trim_end_matches('/')));
    let root = base.to_file_path().ok()?;
    let target = target.replace('\\', "/");
    let url = if target.as_bytes().get(1) == Some(&b':') {
        let mut url = url::Url::parse("file:///").ok()?;
        url.set_path(&target);
        url
    } else {
        base.join(&target).ok()?
    };
    if url.scheme() != "file" {
        return None;
    }
    let path = url.to_file_path().ok()?;
    if path.to_str()?.contains('\0') {
        return None;
    }
    if let Ok(relative) = path.strip_prefix(root) {
        Some(Target::Workspace {
            path: relative.to_str()?.replace('\\', "/"),
            line,
        })
    } else {
        Some(Target::External(path))
    }
}

#[cfg(test)]
pub(crate) fn parse(link: &str) -> Option<(&str, Option<usize>)> {
    let (path, line) = parts(link)?;
    let path = path.strip_prefix("./").unwrap_or(path);
    if path.is_empty()
        || path.starts_with('/')
        || path.contains([':', '\\', '\0'])
        || path.split('/').any(|part| part == "..")
    {
        return None;
    }
    Some((path, line))
}

fn parts(link: &str) -> Option<(&str, Option<usize>)> {
    let (target, fragment) = link
        .split_once('#')
        .map_or((link, None), |(path, part)| (path, Some(part)));
    // `#L12`, `#L12-L20` and `#L12C4` name a line; `#Licensing` is a heading.
    let mut line = match fragment
        .and_then(|part| part.strip_prefix('L'))
        .filter(|value| value.starts_with(|c: char| c.is_ascii_digit() || c == '-'))
    {
        Some(value) => {
            let end = value
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(value.len());
            if !(value[end..].is_empty() || value[end..].starts_with(['-', 'C'])) {
                return None;
            }
            Some(parse_line(&value[..end])?)
        }
        None => None,
    };
    let target = target.split('?').next()?;
    let target = match target.rsplit_once(':') {
        Some((path, suffix)) if digits(suffix) => {
            if line.is_some() {
                return None;
            }
            // `path:line:column` keeps the line.
            let (path, suffix) = match path.rsplit_once(':') {
                Some((file, value)) if digits(value) => (file, value),
                _ => (path, suffix),
            };
            line = Some(parse_line(suffix)?);
            path
        }
        _ => target,
    };
    if target.is_empty() || target.contains('\0') {
        return None;
    }
    Some((target, line))
}

fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|value| value.is_ascii_digit())
}

fn parse_line(value: &str) -> Option<usize> {
    value
        .parse::<u32>()
        .ok()
        .filter(|line| *line > 0)
        .map(|line| line as usize)
}

#[cfg(test)]
mod tests;
