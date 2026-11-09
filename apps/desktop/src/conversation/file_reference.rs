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
    let root = node_path(root);
    let mut base = url::Url::parse("file:///").ok()?;
    base.set_path(&format!(
        "{}/",
        root.trim_end_matches('/').replace('%', "%25")
    ));
    let root = format!("{}/", file_path(&base)?.trim_end_matches('/'));
    let target = node_path(target);
    let url = if target.as_bytes().get(1) == Some(&b':') || target.starts_with("//") {
        let mut url = url::Url::parse("file:///").ok()?;
        url.set_path(&target);
        url
    } else {
        base.join(&target).ok()?
    };
    let path = file_path(&url)?;
    if let Some(relative) = path
        .strip_prefix(root.as_str())
        .or_else(|| (path == root.trim_end_matches('/')).then_some(""))
    {
        Some(Target::Workspace {
            path: relative.to_owned(),
            line,
        })
    } else {
        Some(Target::External(path.into()))
    }
}

fn node_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    if let Some(path) = path.strip_prefix("//?/UNC/") {
        format!("//{path}")
    } else {
        path.strip_prefix("//?/").unwrap_or(&path).to_owned()
    }
}

fn file_path(url: &url::Url) -> Option<String> {
    if url.scheme() != "file" || url.host_str().is_some() {
        return None;
    }
    let path = percent_encoding::percent_decode_str(url.path())
        .decode_utf8()
        .ok()?
        .replace('\\', "/");
    if path.contains('\0') {
        return None;
    }
    let drive = path.as_bytes().get(1).is_some_and(u8::is_ascii_alphabetic)
        && path.as_bytes().get(2) == Some(&b':')
        && (path.len() == 3 || path.as_bytes().get(3) == Some(&b'/'));
    let network = path.starts_with("//");
    let volume = if network { 2 } else { usize::from(drive) };
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.len() > volume {
                    parts.pop();
                }
            }
            part => parts.push(part),
        }
    }
    let mut path = parts.join("/");
    if drive {
        path[..1].make_ascii_uppercase();
        if parts.len() == 1 {
            path.push('/');
        }
    } else if network {
        path.insert_str(0, "//");
    } else {
        path.insert(0, '/');
    }
    Some(path)
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
    let prefix = if target.starts_with(r"\\?\") || target.starts_with("//?/") {
        4
    } else {
        0
    };
    let target = target[prefix..]
        .find('?')
        .map_or(target, |offset| &target[..prefix + offset]);
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
