//! Foreign package metadata is inspected without executing its hooks or normalizing its manifest.
use super::*;
use sailry_protocol::plugin::catalog::{Details, External, Format, Metadata, SkillCollection};

impl Host {
    pub(in crate::plugins) async fn inspect_catalog_source(
        &self,
        source: &Source,
        stop: CancellationToken,
    ) -> Result<Details, Fault> {
        let selected = source::Selection::parse(source)?;
        let (archive, resolved) = self
            .source_archive(&selected, None, stop.clone(), None)
            .await?;
        tokio::task::spawn_blocking(move || {
            let (_temporary, root, links) = archive::unpack_source(archive, &stop, 10_000)?;
            inspect(&root, &selected.path, &links, resolved, stop)
        })
        .await
        .map_err(|_| github::unavailable())?
    }
}

fn inspect(
    root: &Path,
    path: &str,
    links: &[String],
    source: Resolved,
    stop: CancellationToken,
) -> Result<Details, Fault> {
    let directory = crate::files::path::root(&package_root(root, path, links)?)?;
    let root_manifest = read(&directory, "plugin.json")?;
    // An explicitly declared portable manifest remains subject to its complete validator.
    // Directory-provided synthetic manifests never replace the repository's own bytes.
    if root_manifest
        .as_ref()
        .is_some_and(|value| value.get("$schema").is_some())
    {
        return inspect_root(root, path, links, stop).map(|info| Details::Plugin(Box::new(info)));
    }
    let mut formats = Vec::new();
    let mut metadata = None;
    for (name, format) in [
        (".codex-plugin", Format::Codex),
        (".claude-plugin", Format::ClaudeCode),
    ] {
        let components = crate::files::path::components(name, false)?;
        let foreign = match crate::files::path::descend(
            directory.try_clone().map_err(io_error)?,
            &components,
        ) {
            Ok(directory) => read(&directory, "plugin.json")?,
            Err(error) if error.code == ErrorCode::NotFound => None,
            Err(error) => return Err(error),
        };
        if let Some(value) = foreign {
            let value = public_metadata(&value)?;
            metadata.get_or_insert(value);
            formats.push(format);
        }
    }
    let Some(metadata) = metadata else {
        // Missing or invalid portable declarations are not silently accepted as foreign packages.
        return inspect_root(root, path, links, stop).map(|info| Details::Plugin(Box::new(info)));
    };
    let mut skills = Vec::new();
    crate::plugins::skill_sources::package::scan(
        &directory,
        &source.repository,
        path,
        &mut skills,
        &stop,
        0,
    )?;
    if skills.is_empty() {
        Ok(Details::Unsupported(Box::new(External {
            source,
            metadata,
            formats,
        })))
    } else {
        Ok(Details::Skills(Box::new(SkillCollection {
            source,
            metadata,
            skills,
            unsupported: formats,
        })))
    }
}

fn read(directory: &cap_std::fs::Dir, path: &str) -> Result<Option<serde_json::Value>, Fault> {
    package::read(directory, path, manifest::MAX_BYTES)?
        .map(|bytes| {
            serde_json::from_slice(&bytes)
                .map_err(|_| Fault::invalid("invalid repository plugin metadata"))
        })
        .transpose()
}

fn public_metadata(value: &serde_json::Value) -> Result<Metadata, Fault> {
    let text = |key: &str| -> Result<Option<String>, Fault> {
        value
            .get(key)
            .map(|value| {
                value
                    .as_str()
                    .filter(|text| !text.trim().is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| Fault::invalid("invalid repository plugin metadata"))
            })
            .transpose()
    };
    let name = text("name")?.ok_or_else(|| Fault::invalid("repository plugin name is required"))?;
    let homepage = text("homepage")?;
    if let Some(homepage) = &homepage {
        let url = url::Url::parse(homepage)
            .map_err(|_| Fault::invalid("invalid repository plugin homepage"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(Fault::invalid("invalid repository plugin homepage"));
        }
    }
    let author = match value.get("author") {
        Some(serde_json::Value::Object(author)) => author
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        Some(serde_json::Value::String(author)) => Some(author.clone()),
        None | Some(serde_json::Value::Null) => None,
        _ => return Err(Fault::invalid("invalid repository plugin author")),
    };
    Ok(Metadata {
        name,
        version: text("version")?,
        description: text("description")?,
        homepage,
        license: text("license")?,
        author,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Resolved) {
        let directory = tempfile::tempdir().unwrap();
        let source = Resolved {
            repository: "https://github.com/example/foreign.git".into(),
            git_ref: "HEAD".into(),
            commit: "a".repeat(40),
        };
        (directory, source)
    }
    #[test]
    fn reads_foreign_skills_without_synthetic_manifests() {
        let (directory, source) = fixture();
        let root = directory.path().canonicalize().unwrap();
        let root = root.as_path();
        std::fs::create_dir_all(root.join(".codex-plugin")).unwrap();
        std::fs::create_dir_all(root.join("skills/review")).unwrap();
        std::fs::write(root.join("plugin.json"), r#"{"name":"foreign"}"#).unwrap();
        std::fs::write(root.join(".codex-plugin/plugin.json"), r#"{"name":"foreign","version":"1.2.3","author":{"name":"Example"},"hooks":{"danger":"never execute"}}"#).unwrap();
        std::fs::write(
            root.join("skills/review/SKILL.md"),
            "---\nname: review\ndescription: Review changes\n---\nReview the supplied changes\n",
        )
        .unwrap();
        let Details::Skills(details) =
            inspect(root, "", &[], source.clone(), CancellationToken::new()).unwrap()
        else {
            panic!("skills expected")
        };
        assert_eq!(details.source, source);
        assert_eq!(details.metadata.name, "foreign");
        assert_eq!(details.metadata.version.as_deref(), Some("1.2.3"));
        assert_eq!(details.unsupported, [Format::Codex]);
        assert_eq!(details.skills.len(), 1);
        assert_eq!(details.skills[0].path, "skills/review");
        std::fs::write(
            root.join("plugin.json"),
            r#"{"$schema":"invalid","name":"foreign"}"#,
        )
        .unwrap();
        assert_eq!(
            inspect(root, "", &[], source, CancellationToken::new())
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    #[test]
    fn preserves_unsupported_format_metadata() {
        let (directory, source) = fixture();
        std::fs::create_dir_all(directory.path().join(".claude-plugin")).unwrap();
        std::fs::write(
            directory.path().join(".claude-plugin/plugin.json"),
            r#"{"name":"foreign","description":"A real foreign package"}"#,
        )
        .unwrap();
        let Details::Unsupported(details) = inspect(
            &directory.path().canonicalize().unwrap(),
            "",
            &[],
            source.clone(),
            CancellationToken::new(),
        )
        .unwrap() else {
            panic!("unsupported format expected")
        };
        assert_eq!(details.source, source);
        assert_eq!(details.formats, [Format::ClaudeCode]);
        assert_eq!(
            details.metadata.description.as_deref(),
            Some("A real foreign package")
        );
    }
}
