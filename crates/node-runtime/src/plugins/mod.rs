//! Node-owned immutable packages. Behavioral references: Code 67ae9fa0
//! production/storage/packages.rs, production/mcp.rs and Platform 727ce0a harbor-agent-plugins
//! (Apache-2.0). No historical UI or parallel execution host is imported.
mod archive;
pub(crate) mod authorization;
mod bundled;
pub(crate) mod catalog;
mod collection;
pub(crate) mod conversation;
pub(crate) mod github;
pub(crate) mod http;
pub(crate) mod icon;
mod manifest;
pub(crate) mod mcp;
mod package;
mod repository;
pub(crate) mod resources;
pub(crate) mod script;
pub(crate) mod settings;
mod skill_sources;
mod skills;
mod sources;
pub(crate) mod standalone_mcp;
pub(crate) mod storage;
pub(crate) mod tools;
mod updates;

use sailry_protocol::{
    ErrorCode, Fault,
    plugin::{Info, Summary},
};
use std::path::{Path, PathBuf};

pub(crate) use bundled::DEFAULTS;
pub(crate) use manifest::validate_name;

#[derive(Clone)]
pub(crate) struct Host {
    profile: Option<PathBuf>,
    pub(crate) scripts: std::sync::Arc<tokio::sync::Semaphore>,
    reads: std::sync::Arc<tokio::sync::Semaphore>,
    authorizations: std::sync::Arc<authorization::Clients>,
    github: github::Github,
}

impl Host {
    pub(crate) fn new(profile: Option<PathBuf>) -> Self {
        Self {
            profile,
            scripts: std::sync::Arc::new(tokio::sync::Semaphore::new(2)),
            reads: std::sync::Arc::new(tokio::sync::Semaphore::new(2)),
            authorizations: Default::default(),
            github: Default::default(),
        }
    }

    /// Called only on the existing resource worker, never on the storage or UI thread.
    pub(crate) fn install(&self, worktree: &Path, path: &str, name: &str) -> Result<Info, Fault> {
        use crate::files::path::{components, descend, root};
        use cap_fs_ext::DirExt;
        validate_name(name)?;
        let profile = self
            .profile
            .as_deref()
            .ok_or_else(|| Fault::new(ErrorCode::Unavailable, "plugin storage is unavailable"))?;
        let resolved = worktree.join(path).canonicalize().map_err(io_error)?;
        if resolved.starts_with(profile) || profile.starts_with(&resolved) {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "plugin source overlaps Node storage",
            ));
        }
        let source = descend(root(worktree)?, &components(path, true)?)?;
        let bytes = package::read(&source, "plugin.json", 64 * 1024)?
            .ok_or_else(|| invalid("plugin.json is required"))?;
        let manifest = manifest::parse(&bytes)?;
        if manifest.name != name {
            return Err(invalid("plugin name does not match the selected package"));
        }
        let profile = root(profile)?;
        let packages = directory(&directory(&profile, "plugins")?, "packages")?;
        let staging = cap_tempfile::TempDir::new_in(&packages).map_err(io_error)?;
        let result = (|| {
            staging.create_dir("package").map_err(io_error)?;
            let staged = staging.open_dir_nofollow("package").map_err(io_error)?;
            let mut tree = package::Tree::default();
            tree.walk(&source, Some(&staged), "", 0)?;
            let digest = tree.digest();
            // Parse the copied bytes, not a path that may have changed during staging.
            let bytes = package::read(&staged, "plugin.json", 64 * 1024)?
                .ok_or_else(|| invalid("staged plugin manifest is unavailable"))?;
            let manifest = manifest::parse(&bytes)?;
            if manifest.name != name {
                return Err(invalid("plugin manifest changed during installation"));
            }
            let info = describe(&staged, manifest, digest.clone(), tree.issues)?;
            if info.issues.iter().any(|issue| {
                matches!(
                    issue.kind,
                    sailry_protocol::plugin::IssueKind::InvalidSkill
                        | sailry_protocol::plugin::IssueKind::InvalidSkills
                )
            }) {
                return Err(invalid("plugin contains invalid skills"));
            }
            match packages.open_dir_nofollow(&digest) {
                Ok(existing) => {
                    let mut tree = package::Tree::default();
                    tree.walk(&existing, None, "", 0)?;
                    if tree.digest() != digest || !tree.issues.is_empty() {
                        return Err(Fault::new(
                            ErrorCode::Unavailable,
                            "stored plugin package changed",
                        ));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    staging
                        .rename("package", &packages, &digest)
                        .map_err(io_error)?;
                    #[cfg(unix)]
                    packages
                        .try_clone()
                        .and_then(|dir| dir.into_std_file().sync_all())
                        .map_err(io_error)?;
                }
                Err(error) => return Err(io_error(error)),
            }
            Ok(info)
        })();
        staging
            .close()
            .map_err(|_| Fault::new(ErrorCode::Unavailable, "plugin staging cleanup failed"))?;
        result
    }
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}

fn describe(
    source: &cap_std::fs::Dir,
    mut manifest: manifest::Manifest,
    digest: String,
    issues: Vec<sailry_protocol::plugin::Issue>,
) -> Result<Info, Fault> {
    manifest.issues.extend(issues);
    let skills = skills::discover(source, &mut manifest.issues);
    let servers = mcp::discover(source, &mut manifest.issues);
    let settings = settings::discover(
        source,
        manifest.extension.as_ref(),
        &servers,
        &mut manifest.issues,
    );
    let mcp = servers
        .iter()
        .map(|(name, server)| sailry_protocol::plugin::Mcp {
            name: name.clone(),
            transport: server.transport(),
        })
        .collect();
    let icon = icon::read(source, manifest.extension.as_ref(), &mut manifest.issues);
    let info = Info {
        origin: None,
        icon,
        skill: None,
        mcp_source: None,
        extension: manifest.extension,
        summary: Summary {
            name: manifest.name,
            revision: 0,
            digest,
            settings_revision: 0,
            enabled: true,
            version: manifest.version,
            description: manifest.description,
        },
        skills,
        mcp,
        settings,
        issues: manifest.issues,
    };
    if serde_json::to_vec(&info)
        .map_err(|_| invalid("plugin metadata could not be encoded"))?
        .len()
        > 256 * 1024
    {
        return Err(invalid("plugin metadata exceeds the component limit"));
    }
    Ok(info)
}

fn directory(parent: &cap_std::fs::Dir, name: &str) -> Result<cap_std::fs::Dir, Fault> {
    use cap_fs_ext::DirExt;
    match parent.create_dir(name) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(io_error(error)),
    }
    parent.open_dir_nofollow(name).map_err(io_error)
}
fn io_error(_: std::io::Error) -> Fault {
    Fault::new(ErrorCode::Unavailable, "plugin file operation failed")
}

#[cfg(test)]
mod tests;
