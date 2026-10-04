//! Markdown paths and file URLs resolve against the originating worktree.
use std::path::PathBuf;

#[derive(Debug, PartialEq)]
pub(crate) enum Target {
    Workspace { path: String, line: Option<usize> },
    External(PathBuf),
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
    let mut line = match fragment.and_then(|part| part.strip_prefix('L')) {
        Some(value) => Some(parse_line(value)?),
        None => None,
    };
    let target = target.split('?').next()?;
    let target = if let Some((path, suffix)) = target.rsplit_once(':') {
        if !suffix.is_empty() && suffix.bytes().all(|value| value.is_ascii_digit()) {
            if line.is_some() {
                return None;
            }
            line = Some(parse_line(suffix)?);
            path
        } else {
            target
        }
    } else {
        target
    };
    if target.is_empty() || target.contains('\0') {
        return None;
    }
    Some((target, line))
}

fn parse_line(value: &str) -> Option<usize> {
    value
        .parse::<u32>()
        .ok()
        .filter(|line| *line > 0)
        .map(|line| line as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolves_worktree_lines() {
        assert_eq!(parse("src/main.rs#L42"), Some(("src/main.rs", Some(42))));
        assert_eq!(parse("./src/main.rs:42"), Some(("src/main.rs", Some(42))));
        assert_eq!(
            parse("docs/build notes.md#L12"),
            Some(("docs/build notes.md", Some(12)))
        );
        assert_eq!(parse("README.md#setup"), Some(("README.md", None)));
    }
    #[test]
    fn rejects_invalid_targets() {
        for path in [
            "a.rs:0",
            "a.rs#L-1",
            "a.rs#L999999999999999",
            "a.rs:3#L5",
            "../secret",
            "/etc/passwd",
            "C:\\secret",
            "file:///etc/passwd",
            "https://example.test",
        ] {
            assert_eq!(parse(path), None, "{path}");
        }
    }

    #[test]
    fn resolves_urls() {
        for link in [
            "build%20notes.md#L3",
            "/project/build notes.md:3",
            "file:///project/build%20notes.md#L3",
        ] {
            assert_eq!(
                resolve(link, "/project"),
                Some(Target::Workspace {
                    path: "build notes.md".into(),
                    line: Some(3)
                })
            );
        }
        for link in [
            "../outside.png",
            "%2e%2e/outside.png",
            "file:///outside.png",
        ] {
            assert_eq!(
                resolve(link, "/project"),
                Some(Target::External("/outside.png".into()))
            );
        }
        for link in [
            "https://example.test/a.png",
            "file://remote/path",
            "image.png#L0",
            "image.png%00",
            "a.rs:3#L5",
        ] {
            assert_eq!(resolve(link, "/project"), None, "{link}");
        }
    }
}
