use super::*;
pub(in crate::plugins) use crate::plugins::github::source::{commit, name};

pub(in crate::plugins) struct Selection {
    pub repository: String,
    pub git_ref: String,
    pub path: String,
}

impl Selection {
    pub fn parse(source: &Source) -> Result<Self, Fault> {
        let input = source.repository.trim();
        if input.is_empty() || input.len() > 2048 || input.chars().any(char::is_control) {
            return Err(invalid("invalid repository URL"));
        }
        // Preserve GitHub shorthand and tree URLs while acquisition uses Git, not its API.
        if input.starts_with("https://github.com/")
            || (!input.contains(':') && !std::path::Path::new(input).is_absolute())
        {
            let selected = crate::plugins::github::source::Selection::parse(source)?;
            return Ok(Self {
                repository: selected.repository,
                git_ref: selected.git_ref,
                path: selected.path,
            });
        }
        let repository = if std::path::Path::new(input).is_absolute() {
            let url = url::Url::from_file_path(input)
                .map_err(|_| invalid("invalid local Git repository"))?;
            if url.host_str().is_some_and(|host| host != "localhost") {
                return Err(invalid("unsupported local Git repository authority"));
            }
            url.to_string()
        } else if let Ok(url) = url::Url::parse(input) {
            if !matches!(
                url.scheme(),
                "https" | "http" | "git" | "ssh" | "ssh+git" | "git+ssh" | "file"
            ) || url.password().is_some()
                || (matches!(url.scheme(), "http" | "https" | "git") && !url.username().is_empty())
                || url.query().is_some()
                || url.fragment().is_some()
                || (url.scheme() != "file" && url.host_str().is_none())
                || (url.scheme() == "file"
                    && url.host_str().is_some_and(|host| host != "localhost"))
            {
                return Err(invalid(
                    "use a Git repository URL without embedded credentials",
                ));
            }
            url.to_string()
        } else if !input.contains("://")
            && input.contains('@')
            && input.contains(':')
            && !input.contains(char::is_whitespace)
        {
            let (host, path) = input.split_once(':').unwrap();
            if host.is_empty() || path.is_empty() || host.contains('/') || path.contains('\\') {
                return Err(invalid("invalid SSH Git repository"));
            }
            input.to_owned()
        } else {
            return Err(invalid("a Git repository URL is required"));
        };
        let git_ref = source
            .git_ref
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("HEAD")
            .to_owned();
        crate::plugins::github::source::reference(&git_ref)?;
        let path = source
            .path
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .to_owned();
        crate::plugins::github::source::directory(&path)?;
        Ok(Self {
            repository,
            git_ref,
            path,
        })
    }

    pub fn resolved(source: &Resolved, path: &str) -> Result<Self, Fault> {
        let selected = Self::parse(&Source {
            repository: source.repository.clone(),
            git_ref: Some(source.git_ref.clone()),
            path: Some(path.to_owned()),
        })?;
        if selected.repository != source.repository || !commit(&source.commit) {
            return Err(invalid("a resolved Git commit is required"));
        }
        Ok(selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_git_transports_and_preserves_selection() {
        for repository in [
            "https://example.invalid/team/skills.git",
            "git://example.invalid/team/skills.git",
            "ssh://git@example.invalid/team/skills.git",
            "ssh+git://git@example.invalid/team/skills.git",
            "git+ssh://git@example.invalid/team/skills.git",
            "git@example.invalid:team/skills.git",
            "file:///tmp/skills.git",
        ] {
            let selected = Selection::parse(&Source {
                repository: repository.into(),
                git_ref: Some("release/v1".into()),
                path: Some("skills/review".into()),
            })
            .unwrap();
            assert_eq!(selected.repository, repository);
            assert_eq!(selected.git_ref, "release/v1");
            assert_eq!(selected.path, "skills/review");
        }
        let selected = Selection::parse(&Source {
            repository: "https://github.com/Owner/Repo/tree/release/v1/skills/review".into(),
            git_ref: Some("release/v1".into()),
            path: None,
        })
        .unwrap();
        assert_eq!(selected.repository, "https://github.com/owner/repo.git");
        assert_eq!(selected.path, "skills/review");
    }
    #[test]
    fn rejects_embedded_credentials_and_unconfined_paths() {
        for repository in [
            "https://user:secret@example.invalid/repo.git",
            "https://user@example.invalid/repo.git",
            "ssh://user:secret@example.invalid/repo.git",
            "ssh+git://user:secret@example.invalid/repo.git",
            "git://user@example.invalid/repo.git",
            "git://user:secret@example.invalid/repo.git",
            "git+ssh://git@example.invalid/repo.git?token=value",
            "git+ssh://git@example.invalid/repo.git#branch",
            "ext::command",
            "https://example.invalid/repo.git?token=value",
            "file://server/path/repo.git",
            "file://user:secret@example.invalid/repo.git",
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
        assert!(
            Selection::parse(&Source {
                repository: "https://example.invalid/repo.git".into(),
                git_ref: None,
                path: Some("../outside".into())
            })
            .is_err()
        );
    }

    #[test]
    fn normalizes_absolute_local_paths() {
        let directory = tempfile::tempdir().unwrap();
        let selected = Selection::parse(&Source {
            repository: directory.path().to_str().unwrap().into(),
            git_ref: None,
            path: None,
        })
        .unwrap();
        assert_eq!(
            selected.repository,
            url::Url::from_file_path(directory.path())
                .unwrap()
                .to_string()
        );
    }
}
