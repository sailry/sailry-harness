//! Repository packages ship as ordinary installable assets.
use super::*;
include!(concat!(env!("OUT_DIR"), "/bundled_plugins.rs"));

/// Initial distribution only; restarting never reinstalls removed packages.
pub(crate) const DEFAULTS: &[(&str, bool)] = &[
    ("reminders", true),
    ("scheduled-tasks", true),
    ("web-search", true),
    ("progress", true),
    ("goals", true),
    ("commands", true),
    ("external-browser", false),
    ("browser", true),
    ("computer", false),
    ("media", true),
    ("brief", true),
    ("memory", true),
    ("files", true),
    ("code", true),
    ("delegation", true),
    ("statistics", true),
    ("provider-status", true),
    ("openai-auth", true),
    ("claude-auth", true),
    ("databases", true),
    ("ssh", true),
    ("worktrees", true),
    ("git", true),
    ("context7", true),
    ("github", true),
    ("code-review", true),
    ("office", true),
];

/// Default ownership for credentials saved before package authentication bindings.
pub(crate) const AUTHENTICATIONS: &[(sailry_protocol::Authentication, &str, &str)] = &[
    (
        sailry_protocol::Authentication::ChatGpt,
        "openai-auth",
        "siwc",
    ),
    (
        sailry_protocol::Authentication::ChatGptOAuth,
        "openai-auth",
        "codex",
    ),
    (
        sailry_protocol::Authentication::ClaudeOAuth,
        "claude-auth",
        "subscription",
    ),
];

pub(super) fn entries() -> Result<Vec<sailry_protocol::plugin::catalog::Entry>, Fault> {
    PACKAGES
        .iter()
        .map(|(name, files)| {
            let bytes = files
                .iter()
                .find(|(path, _)| *path == "plugin.json")
                .unwrap()
                .1;
            let manifest = manifest::parse(bytes)?;
            let icon = manifest
                .extension
                .as_ref()
                .and_then(|extension| extension.icon.as_ref())
                .and_then(|path| files.iter().find(|(name, _)| *name == path.as_str()))
                .and_then(|(_, bytes)| super::icon::encode(bytes));
            let mut entry = catalog::entry(name, manifest);
            entry.icon = icon;
            entry.release_notes = notes::read(entry.version.as_deref(), |name| {
                Ok(files
                    .iter()
                    .find(|(path, _)| *path == name)
                    .map(|(_, bytes)| bytes.to_vec()))
            })?;
            entry.bundled = true;
            Ok(entry)
        })
        .collect()
}

pub(super) fn skills() -> Result<Vec<sailry_protocol::plugin::skillcatalog::Entry>, Fault> {
    let mut entries = vec![];
    for (package, files) in PACKAGES {
        for (path, bytes) in *files {
            let Some(name) = path
                .strip_prefix("skills/")
                .and_then(|path| path.strip_suffix("/SKILL.md"))
            else {
                continue;
            };
            if name.contains('/') {
                continue;
            }
            let text =
                std::str::from_utf8(bytes).map_err(|_| Fault::invalid("invalid bundled skill"))?;
            let skill = super::skills::parse(name, text)?;
            entries.push(catalog::skills::entry(package, skill, true)?);
        }
    }
    Ok(entries)
}

pub(in crate::plugins) fn document(
    package: &str,
    skill: &str,
) -> Result<sailry_protocol::plugin::skills::Document, Fault> {
    let (_, files) = PACKAGES
        .iter()
        .find(|(name, _)| *name == package)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "bundled plugin is unavailable"))?;
    let path = format!("skills/{skill}/SKILL.md");
    let (_, bytes) = files
        .iter()
        .find(|(name, _)| *name == path)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "bundled skill is unavailable"))?;
    let document = super::skill_sources::document::parse(bytes)?;
    if document.skill.name != skill {
        return Err(Fault::invalid("bundled skill identity does not match"));
    }
    Ok(document)
}

impl Host {
    pub(crate) fn bundled_version(name: &str) -> Result<Option<String>, Fault> {
        let (_, files) = PACKAGES
            .iter()
            .find(|(id, _)| *id == name)
            .ok_or_else(|| Fault::new(ErrorCode::NotFound, "bundled plugin is unavailable"))?;
        let bytes = files
            .iter()
            .find(|(path, _)| *path == "plugin.json")
            .unwrap()
            .1;
        let manifest = manifest::parse(bytes)?;
        if manifest.name != name {
            return Err(Fault::invalid("bundled plugin identity does not match"));
        }
        Ok(manifest.version)
    }

    pub(crate) fn bundled_newer(name: &str, current: Option<&str>) -> Result<bool, Fault> {
        let Ok(current) = updates::version(current) else {
            return Ok(false);
        };
        let available = Self::bundled_version(name)?;
        Ok(updates::version(available.as_deref())?
            .cmp_precedence(&current)
            .is_gt())
    }

    #[cfg(test)]
    pub(crate) fn bundled_digest(name: &str) -> Result<String, Fault> {
        let (_, files) = PACKAGES
            .iter()
            .find(|(id, _)| *id == name)
            .ok_or_else(|| Fault::new(ErrorCode::NotFound, "bundled plugin is unavailable"))?;
        Ok(package::digest(files))
    }

    fn bundled_source(name: &str) -> Result<tempfile::TempDir, Fault> {
        let (_, files) = PACKAGES
            .iter()
            .find(|(id, _)| *id == name)
            .ok_or_else(|| Fault::new(ErrorCode::NotFound, "bundled plugin is unavailable"))?;
        let temporary = tempfile::tempdir().map_err(io_error)?;
        for (path, bytes) in *files {
            let path = temporary.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).map_err(io_error)?;
            std::fs::write(path, bytes).map_err(io_error)?;
        }
        Ok(temporary)
    }

    pub(crate) fn install_bundled(&self, name: &str) -> Result<Info, Fault> {
        let (_, files) = PACKAGES
            .iter()
            .find(|(id, _)| *id == name)
            .ok_or_else(|| Fault::new(ErrorCode::NotFound, "bundled plugin is unavailable"))?;
        self.stage(name, |staged| package::write_embedded(staged, files))
    }

    pub(crate) fn inspect_bundled(
        &self,
        name: &str,
        stop: sailry_link::CancellationToken,
    ) -> Result<Info, Fault> {
        let temporary = Self::bundled_source(name)?;
        self.inspect(
            &temporary.path().canonicalize().map_err(io_error)?,
            "",
            name,
            stop,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installs_every_shipped_package() {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        std::fs::create_dir(&profile).unwrap();
        let host = Host::new(Some(profile.canonicalize().unwrap()));
        for entry in entries().unwrap() {
            assert!(
                entry
                    .description("en")
                    .is_some_and(|description| !description.is_empty()),
                "{}",
                entry.name
            );
            assert!(
                entry
                    .description("zh-CN")
                    .is_some_and(|description| !description.is_empty()),
                "{}",
                entry.name
            );
            assert!(
                entry.description_locales.contains_key("zh-CN"),
                "{}",
                entry.name
            );
            let info = host.install_bundled(&entry.name).unwrap();
            assert_eq!(info.summary.name, entry.name);
            assert!(info.summary.enabled);
            assert_eq!(
                info.summary.digest,
                Host::bundled_digest(&entry.name).unwrap()
            );
            assert!(info.issues.is_empty(), "{}: {:?}", entry.name, info.issues);
            let glyph = entry
                .display
                .as_ref()
                .unwrap_or_else(|| panic!("{} has no display declaration", entry.name))
                .icon
                .as_ref()
                .unwrap_or_else(|| panic!("{} has no display icon", entry.name));
            assert!(glyph.valid(), "{}", entry.name);
            let extension = info.extension.as_ref().unwrap();
            assert_eq!(
                extension.display.as_ref().unwrap().icon.as_ref(),
                Some(glyph)
            );
            if let Some(navigation) = extension
                .desktop
                .as_ref()
                .and_then(|desktop| desktop.navigation.as_ref())
            {
                assert_eq!(navigation.icon.as_ref(), Some(glyph));
            }
            assert!(entry.icon.is_none(), "{}", entry.name);
            assert_eq!(info.icon, entry.icon);
            assert!(info.issues.is_empty(), "{}: {:?}", entry.name, info.issues);
        }
        assert!(host.install_bundled("unknown").is_err());
    }
}
