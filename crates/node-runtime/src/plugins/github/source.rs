use super::*;
use sailry_protocol::plugin::skills::Source;

pub(crate) struct Selection {
    pub repository: String,
    #[cfg(any(test, feature = "test-support"))]
    pub owner: String,
    #[cfg(any(test, feature = "test-support"))]
    pub repo: String,
    pub git_ref: String,
    pub path: String,
}

impl Selection {
    pub fn parse(source: &Source) -> Result<Self, Fault> {
        let input = source.repository.trim();
        if input.len() > 2048 || input.contains('\\') || input.chars().any(char::is_control) {
            return Err(invalid("invalid plugin repository URL"));
        }
        // URL parsing normalizes dot segments; reject them before that loses intent.
        for segment in input.split('/') {
            if matches!(decode(segment)?.as_str(), "." | "..") {
                return Err(invalid("repository path contains a dot segment"));
            }
        }
        let url = if input.contains("://") {
            url::Url::parse(input)
        } else {
            url::Url::parse(&format!("https://github.com/{input}"))
        }
        .map_err(|_| invalid("invalid GitHub repository"))?;
        if url.scheme() != "https"
            || url.host_str() != Some("github.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid("a public GitHub repository URL is required"));
        }
        let parts = url
            .path_segments()
            .ok_or_else(|| invalid("invalid repository path"))?
            .filter(|part| !part.is_empty())
            .map(decode)
            .collect::<Result<Vec<_>, _>>()?;
        if parts.len() < 2 || !slug(&parts[0]) || !slug(parts[1].trim_end_matches(".git")) {
            return Err(invalid("GitHub owner and repository are required"));
        }
        let owner = parts[0].to_ascii_lowercase();
        let repo = parts[1].trim_end_matches(".git").to_ascii_lowercase();
        let mut git_ref = source
            .git_ref
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("HEAD")
            .to_owned();
        let mut path = String::new();
        if parts.len() > 2 {
            if parts[2] != "tree" || parts.len() < 4 {
                return Err(invalid("expected a repository or GitHub tree URL"));
            }
            let tail = parts[3..].join("/");
            if source
                .git_ref
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            {
                path = if tail == git_ref {
                    String::new()
                } else {
                    tail.strip_prefix(&format!("{git_ref}/"))
                        .ok_or_else(|| invalid("GitHub tree URL and selected revision differ"))?
                        .to_owned()
                };
            } else {
                git_ref = parts[3].clone();
                path = parts[4..].join("/");
            }
        }
        if let Some(selected) = source
            .path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            if !path.is_empty() && path != selected {
                return Err(invalid("GitHub tree URL and selected directory differ"));
            }
            path = selected.to_owned();
        }
        reference(&git_ref)?;
        directory(&path)?;
        Ok(Self {
            repository: format!("https://github.com/{owner}/{repo}.git"),
            #[cfg(any(test, feature = "test-support"))]
            owner,
            #[cfg(any(test, feature = "test-support"))]
            repo,
            git_ref,
            path,
        })
    }
}

pub(crate) fn directory(path: &str) -> Result<(), Fault> {
    if path.len() > 1024 {
        return Err(invalid("plugin directory is too long"));
    }
    crate::files::path::components(path, true).map(|_| ())
}

pub(crate) fn commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn reference(value: &str) -> Result<(), Fault> {
    if value.is_empty()
        || value.len() > 256
        || value.starts_with('-')
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains("..")
        || value.contains("@{")
        || value
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace() || "~^:?*[\\".contains(ch))
    {
        return Err(invalid("invalid Git revision"));
    }
    Ok(())
}

fn slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

fn decode(value: &str) -> Result<String, Fault> {
    let mut bytes = Vec::new();
    let mut input = value.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let hi = input.next().and_then(|v| (v as char).to_digit(16));
            let lo = input.next().and_then(|v| (v as char).to_digit(16));
            bytes.push(match (hi, lo) {
                (Some(hi), Some(lo)) => (hi * 16 + lo) as u8,
                _ => return Err(invalid("invalid URL escape")),
            });
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).map_err(|_| invalid("repository path must be UTF-8"))
}

pub(crate) fn name(repository: &str, path: &str) -> String {
    let mut hash = blake3::Hasher::new_derive_key("Sailry standalone skill v1");
    hash.update(repository.as_bytes());
    hash.update(&[0]);
    hash.update(path.as_bytes());
    format!("skill-{}", &hash.finalize().to_hex()[..32])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonicalizes_urls() {
        for repository in [
            "Owner/Repo",
            "https://github.com/Owner/Repo.git",
            "https://github.com/Owner/Repo/",
        ] {
            let selected = Selection::parse(&Source {
                repository: repository.into(),
                git_ref: None,
                path: None,
            })
            .unwrap();
            assert_eq!(selected.repository, "https://github.com/owner/repo.git");
            assert_eq!(selected.git_ref, "HEAD");
        }
        let selected = Selection::parse(&Source {
            repository: "https://github.com/Owner/Repo/tree/main/skills/review".into(),
            git_ref: None,
            path: None,
        })
        .unwrap();
        assert_eq!(selected.path, "skills/review");
        assert_eq!(selected.git_ref, "main");
        let selected = Selection::parse(&Source {
            repository: "https://github.com/owner/repo/tree/release/v1/skills/review".into(),
            git_ref: Some("release/v1".into()),
            path: None,
        })
        .unwrap();
        assert_eq!(selected.path, "skills/review");
    }
    #[test]
    fn rejects_unconfined_sources() {
        for repository in [
            "https://example.com/o/r",
            "http://github.com/o/r",
            "https://user@github.com/o/r",
            "https://github.com/o/r?token=secret",
            "https://github.com/o/r/blob/main/SKILL.md",
            "https://github.com/o/r/tree/main/../private",
            "https://github.com/o/r/tree/main/%2e%2e/private",
            "https://github.com/o/r/tree/main/..\\private",
        ] {
            assert!(
                Selection::parse(&Source {
                    repository: repository.into(),
                    git_ref: None,
                    path: None
                })
                .is_err()
            );
        }
        for path in ["../escape", "/tmp", "a/../b", "a\\b", "a\0b"] {
            assert!(
                Selection::parse(&Source {
                    repository: "owner/repo".into(),
                    git_ref: None,
                    path: Some(path.into())
                })
                .is_err()
            );
        }
        for git_ref in ["--upload-pack=bad", "a..b", "main^", "a\nb"] {
            assert!(
                Selection::parse(&Source {
                    repository: "owner/repo".into(),
                    git_ref: Some(git_ref.into()),
                    path: None
                })
                .is_err()
            );
        }
    }
}
