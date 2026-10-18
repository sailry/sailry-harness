use sailry_protocol::{
    Command, Fault, Output,
    plugin::skills::{
        Candidate, Discovery as GitDiscovery, ImportDiscovery, RegistryDiscovery, RegistrySource,
        Source,
    },
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::settings) enum Channel {
    #[default]
    Git,
    Skills,
    Hub,
    Directory,
    Archive,
}

impl Channel {
    pub fn index(self) -> usize {
        match self {
            Self::Git => 0,
            Self::Skills => 1,
            Self::Hub => 2,
            Self::Directory => 3,
            Self::Archive => 4,
        }
    }

    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Skills,
            2 => Self::Hub,
            3 => Self::Directory,
            4 => Self::Archive,
            _ => Self::Git,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Git => "skill_source_git",
            Self::Skills => "skill_source_skills",
            Self::Hub => "skill_source_hub",
            Self::Directory => "skill_source_directory",
            Self::Archive => "skill_source_archive",
        }
    }

    /// Remote sources explain their input; local sources use the path picker.
    pub fn help(self) -> Option<&'static str> {
        match self {
            Self::Git => Some("skill_source_git_help"),
            Self::Skills => Some("skill_source_skills_help"),
            Self::Hub => Some("skill_source_hub_help"),
            Self::Directory | Self::Archive => None,
        }
    }

    pub fn local(self) -> Option<usize> {
        match self {
            Self::Directory => Some(0),
            Self::Archive => Some(1),
            _ => None,
        }
    }
}

pub(super) enum Selection {
    Git { source: Source },
    Registry(RegistrySource),
}

pub(super) enum Discovery {
    Git(GitDiscovery),
    Registry(RegistryDiscovery),
    Imported(ImportDiscovery),
}

impl Discovery {
    pub fn skills(&self) -> &[Candidate] {
        match self {
            Self::Git(value) => &value.skills,
            Self::Registry(value) => &value.skills,
            Self::Imported(value) => &value.skills,
        }
    }

    pub fn install(&self, index: usize, expected_revision: u64) -> Option<Command> {
        let candidate = self.skills().get(index)?;
        Some(match self {
            Self::Git(value) => Command::InstallSkill {
                source: value.source.clone(),
                path: candidate.path.clone(),
                name: candidate.name.clone(),
                expected_revision,
            },
            Self::Registry(value) => Command::InstallRegistrySkill {
                source: value.source.clone(),
                name: candidate.name.clone(),
                expected_revision,
            },
            // Each imported candidate gets a fresh upload lease from the same
            // immutable controller snapshot before durable installation.
            Self::Imported(_) => return None,
        })
    }
}

impl Selection {
    pub fn git(values: [String; 3]) -> Option<Self> {
        (!values[0].is_empty()).then(|| Self::Git {
            source: Source {
                repository: values[0].clone(),
                git_ref: (!values[1].is_empty()).then(|| values[1].clone()),
                path: (!values[2].is_empty()).then(|| values[2].clone()),
            },
        })
    }

    pub fn skills(input: &str) -> Option<Self> {
        let input = input.trim();
        // Directory links contain ASCII identifiers; rejecting escaped path
        // segments prevents URL normalization from concealing traversal.
        if input.len() > 2048 || input.contains(['%', '\\']) || input.chars().any(char::is_control)
        {
            return None;
        }
        let url = url::Url::parse(input).ok()?;
        if url.scheme() != "https"
            || !matches!(url.host_str(), Some("skills.sh" | "www.skills.sh"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return None;
        }
        let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
        if parts.len() != 3
            || parts.iter().any(|part| {
                part.is_empty()
                    || matches!(*part, "." | "..")
                    || !part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
            })
            || input.split('/').any(|part| matches!(part, "." | ".."))
        {
            return None;
        }
        Some(Self::Git {
            source: Source {
                repository: input.into(),
                git_ref: None,
                path: None,
            },
        })
    }

    pub fn hub(input: &str) -> Option<Self> {
        let input = input.trim();
        if input.len() > 2048 || input.contains('\\') || input.chars().any(char::is_control) {
            return None;
        }
        let url = url::Url::parse(input).ok()?;
        if url.scheme() != "https"
            || !matches!(url.host_str(), Some("skillhub.cn" | "www.skillhub.cn"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.fragment().is_some()
        {
            return None;
        }
        let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
        if !(parts.len() == 2 || parts.len() == 3)
            || parts[0] != "skills"
            || parts[1..].iter().any(|part| {
                part.is_empty()
                    || matches!(*part, "." | "..")
                    || !part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
            })
            || input
                .split('?')
                .next()?
                .split('/')
                .any(|part| matches!(part, "." | ".."))
        {
            return None;
        }
        let mut version = None;
        for (key, value) in url.query_pairs() {
            if key != "version"
                || version.is_some()
                || value.is_empty()
                || value.len() > 128
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.+".contains(&byte))
            {
                return None;
            }
            version = Some(value.into_owned());
        }
        Some(Self::Registry(RegistrySource {
            namespace: (parts.len() == 3).then(|| parts[1].to_owned()),
            slug: parts.last()?.to_string(),
            version,
        }))
    }

    pub fn command(&self) -> Command {
        match self {
            Self::Git { source, .. } => Command::DiscoverSkills {
                source: source.clone(),
            },
            Self::Registry(source) => Command::DiscoverRegistrySkill {
                source: source.clone(),
            },
        }
    }

    pub fn select(&self, output: Output) -> Result<Discovery, Fault> {
        match (self, output) {
            (Self::Registry(source), Output::RegistrySkillDiscovery(discovery)) => {
                if source.slug != discovery.source.slug
                    || source
                        .namespace
                        .as_ref()
                        .is_some_and(|namespace| *namespace != discovery.source.namespace)
                    || source
                        .version
                        .as_ref()
                        .is_some_and(|version| *version != discovery.source.version)
                    || discovery.skills.len() != 1
                {
                    return Err(Fault::invalid(
                        "The registry discovery does not match the requested skill",
                    ));
                }
                Ok(Discovery::Registry(discovery))
            }
            (Self::Git { .. }, Output::SkillDiscovery(discovery)) => Ok(Discovery::Git(discovery)),
            _ => Err(Fault::invalid(
                "The skill discovery response does not match its source",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_links_preserve_repository_identity() {
        for host in ["skills.sh", "www.skills.sh"] {
            let selected =
                Selection::skills(&format!("https://{host}/vercel-labs/skills/find-skills"))
                    .unwrap();
            let Selection::Git { source } = selected else {
                panic!("Git source expected")
            };
            assert_eq!(
                source.repository,
                format!("https://{host}/vercel-labs/skills/find-skills")
            );
            assert_eq!(source.path, None);
        }
    }

    #[test]
    fn rejects_other_origins_and_escaped_paths() {
        for input in [
            "http://skills.sh/a/b/c",
            "https://skills.sh.evil/a/b/c",
            "https://user@skills.sh/a/b/c",
            "https://skills.sh/a/b/c?source=x",
            "https://skills.sh/a/b/%2e%2e/c",
            "https://skills.sh/a/b/../b/c",
            "https://skills.sh/a/b",
            "https://skills.sh/a/b/c/extra",
        ] {
            assert!(Selection::skills(input).is_none(), "{input}");
        }
    }

    fn discovery(entries: &[(&str, &str)]) -> GitDiscovery {
        GitDiscovery {
            source: sailry_protocol::plugin::skills::Resolved {
                repository: "https://github.com/example/skills".into(),
                git_ref: "HEAD".into(),
                commit: "a".repeat(40),
            },
            skills: entries
                .iter()
                .map(|(name, path)| sailry_protocol::plugin::skills::Candidate {
                    name: format!("example/{path}"),
                    path: (*path).into(),
                    skill: sailry_protocol::plugin::Skill {
                        name: (*name).into(),
                        display_name: None,
                        display_locales: Default::default(),
                        description: "Fixture skill".into(),
                        path: "SKILL.md".into(),
                        license: Some("MIT".into()),
                        compatibility: None,
                    },
                })
                .collect(),
        }
    }

    #[test]
    fn preserves_node_discovery_and_frozen_paths() {
        let selection = Selection::skills("https://skills.sh/example/skills/find-skills").unwrap();
        let discovered = discovery(&[("Find Skills", "nested/actual-directory")]);
        let frozen = discovered.clone();
        let selected = selection
            .select(Output::SkillDiscovery(discovered))
            .unwrap();
        let Discovery::Git(selected) = selected else {
            panic!("Git discovery expected")
        };
        assert_eq!(selected, frozen);
        assert_eq!(selected.skills[0].path, "nested/actual-directory");
        assert_eq!(selected.skills[0].skill.license.as_deref(), Some("MIT"));
    }

    #[test]
    fn registry_links_keep_namespace_and_version() {
        let Selection::Registry(source) =
            Selection::hub("https://skillhub.cn/skills/clawhub_steipete/github?version=1.0.0")
                .unwrap()
        else {
            panic!("registry source expected")
        };
        assert_eq!(source.namespace.as_deref(), Some("clawhub_steipete"));
        assert_eq!(source.slug, "github");
        assert_eq!(source.version.as_deref(), Some("1.0.0"));
        let Selection::Registry(source) =
            Selection::hub("https://www.skillhub.cn/skills/github").unwrap()
        else {
            panic!("registry source expected")
        };
        assert_eq!(source.namespace, None);
        assert_eq!(source.version, None);
        for input in [
            "https://skillhub.cn.evil/skills/github",
            "http://skillhub.cn/skills/github",
            "https://user@skillhub.cn/skills/github",
            "https://skillhub.cn/skills/%2e%2e/github",
            "https://skillhub.cn/skills/a/../github",
            "https://skillhub.cn/skills/github?token=secret",
            "https://skillhub.cn/skills/github?version=1&version=2",
            "https://skillhub.cn/skills/a/b/c",
        ] {
            assert!(Selection::hub(input).is_none(), "{input}");
        }
    }

    #[test]
    fn registry_install_preserves_the_resolved_identity() {
        let selection =
            Selection::hub("https://skillhub.cn/skills/verified/example?version=1.2.3").unwrap();
        let resolved = sailry_protocol::plugin::skills::RegistryResolved {
            namespace: "verified".into(),
            slug: "example".into(),
            version: "1.2.3".into(),
            version_id: "immutable-version".into(),
            content_hash: "a".repeat(64),
            package_md5: "b".repeat(32),
            archive_sha256: "c".repeat(64),
        };
        let candidate = discovery(&[("Example", "")]).skills.remove(0);
        let registry = RegistryDiscovery {
            source: resolved.clone(),
            skills: vec![candidate.clone()],
        };
        let selected = selection
            .select(Output::RegistrySkillDiscovery(registry.clone()))
            .unwrap();
        let Command::InstallRegistrySkill {
            source,
            name,
            expected_revision,
        } = selected.install(0, 7).unwrap()
        else {
            panic!("registry installation expected")
        };
        assert_eq!(source, resolved);
        assert_eq!(name, candidate.name);
        assert_eq!(expected_revision, 7);
        let mut wrong = registry.clone();
        wrong.source.namespace = "other-publisher".into();
        assert!(
            selection
                .select(Output::RegistrySkillDiscovery(wrong))
                .is_err()
        );
        let mut wrong = registry;
        wrong.source.version = "1.2.4".into();
        assert!(
            selection
                .select(Output::RegistrySkillDiscovery(wrong))
                .is_err()
        );
        assert!(
            selection
                .select(Output::SkillDiscovery(discovery(&[("Example", "")])))
                .is_err()
        );
    }
}
