//! Repository packages ship as ordinary installable assets.
use super::*;
include!(concat!(env!("OUT_DIR"), "/bundled_plugins.rs"));

/// Initial distribution only; restarting never reinstalls removed packages.
pub(crate) const DEFAULTS: &[(&str, bool)] = &[
    ("reminders", true),
    ("scheduled-tasks", true),
    ("web-search", true),
    ("goals", true),
    ("commands", true),
    ("external-browser", false),
    ("browser", true),
    ("computer", false),
    ("media", true),
    ("memory", true),
    ("files", true),
    ("progress", true),
    ("delegation", true),
    ("statistics", true),
    ("databases", true),
    ("ssh", true),
    ("worktrees", true),
    ("git", true),
    ("context7", true),
    ("github", true),
    ("code-review", true),
    ("office", true),
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
            entry.bundled = true;
            Ok(entry)
        })
        .collect()
}

impl Host {
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
        let temporary = Self::bundled_source(name)?;
        self.install(
            &temporary.path().canonicalize().map_err(io_error)?,
            "",
            name,
        )
    }

    pub(super) fn inspect_bundled(
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
            assert!(!info.summary.digest.is_empty());
            assert!(info.issues.is_empty(), "{}: {:?}", entry.name, info.issues);
            let glyph = entry
                .display
                .as_ref()
                .unwrap_or_else(|| panic!("{} has no display declaration", entry.name))
                .icon
                .as_ref()
                .unwrap_or_else(|| panic!("{} has no display icon", entry.name));
            assert!(matches!(
                glyph,
                sailry_protocol::plugin::desktop::Icon::Name(_)
            ));
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
