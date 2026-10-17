//! Directory links select Git candidates without becoming a separate package origin.
use super::*;

pub(super) fn resolve(source: &Source) -> Result<(Source, Option<String>), Fault> {
    let Ok(url) = url::Url::parse(&source.repository) else {
        return Ok((source.clone(), None));
    };
    if !matches!(url.host_str(), Some("skills.sh" | "www.skills.sh")) {
        return Ok((source.clone(), None));
    }
    let parts = url.path().trim_matches('/').split('/').collect::<Vec<_>>();
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || matches!(*part, "." | "..")
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        })
        || source
            .repository
            .split('/')
            .any(|part| matches!(part, "." | ".."))
        || source.path.is_some()
    {
        return Err(Fault::invalid("invalid skills.sh skill URL"));
    }
    Ok((
        Source {
            repository: format!("https://github.com/{}/{}", parts[0], parts[1]),
            git_ref: source.git_ref.clone(),
            path: None,
        },
        Some(parts[2].to_owned()),
    ))
}

pub(super) fn select(candidates: &mut Vec<Candidate>, name: &str) -> Result<(), Fault> {
    let exact = candidates
        .iter()
        .filter(|candidate| candidate.skill.name == name)
        .count();
    if exact == 1 {
        candidates.retain(|candidate| candidate.skill.name == name);
    } else if exact > 1 {
        return Err(Fault::invalid("skill directory link is ambiguous"));
    } else {
        candidates.retain(|candidate| candidate.path.rsplit('/').next() == Some(name));
        if candidates.len() != 1 {
            return Err(Fault::new(
                ErrorCode::NotFound,
                "linked skill was not found unambiguously",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolves_directory_links() {
        let source = Source {
            repository: "https://skills.sh/owner/repo/analysis".into(),
            git_ref: None,
            path: None,
        };
        let (git, name) = resolve(&source).unwrap();
        assert_eq!(git.repository, "https://github.com/owner/repo");
        assert_eq!(name.as_deref(), Some("analysis"));
        for url in [
            "http://skills.sh/owner/repo/analysis",
            "https://skills.sh/owner/repo/analysis?token=private",
            "https://skills.sh/owner/repo/../analysis",
        ] {
            assert!(
                resolve(&Source {
                    repository: url.into(),
                    ..source.clone()
                })
                .is_err()
            );
        }
    }
}
