//! Node-owned immutable packages. Behavioral references: Code 67ae9fa0
//! production/storage/packages.rs, production/mcp.rs and Platform 727ce0a harbor-agent-plugins
//! (Apache-2.0). No historical UI or parallel execution host is imported.
pub(crate) mod applications;
mod archive;
pub(crate) mod authorization;
mod bundled;
pub(crate) mod cards;
pub(crate) mod catalog;
mod collection;
pub(crate) mod conversation;
pub(crate) mod github;
pub(crate) mod http;
pub(crate) mod icon;
mod manifest;
pub(crate) mod mcp;
pub(crate) mod oauth;
mod package;
pub(crate) mod registration;
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
    pub(crate) applications: std::sync::Arc<applications::Service>,
    github: github::Github,
    official_catalog: std::sync::Arc<catalog::official::Cache>,
    python: std::sync::Arc<tokio::sync::OnceCell<std::sync::Arc<crate::python::Manager>>>,
    python_index: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    python_progress:
        std::sync::Arc<tokio::sync::watch::Sender<Vec<sailry_protocol::python::Preparation>>>,
    python_packages: std::sync::Arc<std::sync::Mutex<Vec<Info>>>,
    #[cfg(any(test, feature = "test-support"))]
    python_runtime: std::sync::Arc<std::sync::Mutex<Option<PathBuf>>>,
}

impl Host {
    pub(crate) async fn display_python(&self, commands: &[&str]) -> Option<PathBuf> {
        if commands.is_empty() {
            return None;
        }
        if let Some(manager) = self.python.get() {
            return Some(manager.python().to_owned());
        }
        #[cfg(any(test, feature = "test-support"))]
        let fixture = self
            .python_runtime
            .lock()
            .expect("Python fixture lock is valid")
            .clone();
        #[cfg(not(any(test, feature = "test-support")))]
        let fixture: Option<PathBuf> = None;
        let directory = fixture.or_else(|| crate::python::Runtime::directory().ok())?;
        let candidate = crate::python::Runtime::interpreter(&directory)
            .canonicalize()
            .ok()?;
        let path = candidate.to_str()?;
        if !commands.iter().any(|command| command.contains(path)) {
            return None;
        }
        // The first call is captured before command preparation. Verify only the
        // existing installation; presentation must not create a manager or venv.
        tokio::task::spawn_blocking(move || {
            crate::python::Runtime::open(&directory).map(|runtime| runtime.python().to_owned())
        })
        .await
        .ok()?
        .ok()
    }

    pub(crate) fn new(profile: Option<PathBuf>) -> Self {
        Self {
            profile,
            scripts: std::sync::Arc::new(tokio::sync::Semaphore::new(2)),
            reads: std::sync::Arc::new(tokio::sync::Semaphore::new(2)),
            authorizations: Default::default(),
            applications: Default::default(),
            github: Default::default(),
            official_catalog: Default::default(),
            python: Default::default(),
            python_index: Default::default(),
            python_progress: std::sync::Arc::new(tokio::sync::watch::channel(Vec::new()).0),
            python_packages: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            python_runtime: Default::default(),
        }
    }

    pub(crate) fn configure_python(&self, index: Option<String>) -> Result<(), Fault> {
        crate::python::validate_index(index.as_deref())?;
        let mut current = self
            .python_index
            .lock()
            .expect("Python configuration lock is valid");
        if let Some(manager) = self.python.get() {
            manager.configure(index.clone())?;
        }
        *current = index;
        Ok(())
    }

    pub(crate) fn python_progress(
        &self,
    ) -> tokio::sync::watch::Receiver<Vec<sailry_protocol::python::Preparation>> {
        self.python_progress.subscribe()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_python_runtime_fixture(&self, path: &Path) -> Result<(), Fault> {
        if self.python.get().is_some() {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "Python runtime is already initialized",
            ));
        }
        if !path.is_absolute() {
            return Err(Fault::invalid(
                "Python fixture requires an absolute runtime directory",
            ));
        }
        *self
            .python_runtime
            .lock()
            .expect("Python fixture lock is valid") = Some(path.to_owned());
        Ok(())
    }

    pub(crate) async fn python(
        &self,
        environment: &crate::process::Environment,
    ) -> Result<std::sync::Arc<crate::python::Manager>, Fault> {
        let manager = self
            .python
            .get_or_try_init(|| async {
                let profile = self.profile.clone().ok_or_else(|| {
                    Fault::new(ErrorCode::NotConfigured, "Python requires a Node profile")
                })?;
                let environment = std::sync::Arc::new(environment.clone());
                let progress = self.python_progress.clone();
                #[cfg(any(test, feature = "test-support"))]
                let fixture = self
                    .python_runtime
                    .lock()
                    .expect("Python fixture lock is valid")
                    .clone();
                let manager = tokio::task::spawn_blocking(move || {
                    #[cfg(any(test, feature = "test-support"))]
                    let runtime = match fixture {
                        Some(path) => crate::python::Runtime::open(&path),
                        None => crate::python::Runtime::bundled(),
                    }?;
                    #[cfg(not(any(test, feature = "test-support")))]
                    let runtime = crate::python::Runtime::bundled()?;
                    crate::python::Manager::with_progress(profile, runtime, environment, progress)
                })
                .await
                .map_err(|_| {
                    Fault::new(ErrorCode::Unavailable, "Python runtime preparation failed")
                })??;
                manager.configure(
                    self.python_index
                        .lock()
                        .expect("Python configuration lock is valid")
                        .clone(),
                )?;
                Ok::<_, Fault>(std::sync::Arc::new(manager))
            })
            .await
            .cloned()?;
        // Configuration may change while the first runtime is being verified.
        manager.configure(
            self.python_index
                .lock()
                .expect("Python configuration lock is valid")
                .clone(),
        )?;
        Ok(manager)
    }

    pub(crate) async fn command_python(
        &self,
        environment: &crate::process::Environment,
        session: sailry_protocol::SessionId,
        skills: &[crate::python::Access],
        stop: &sailry_link::CancellationToken,
    ) -> Result<Option<crate::python::Execution>, Fault> {
        #[cfg(any(test, feature = "test-support"))]
        if self
            .python_runtime
            .lock()
            .expect("Python fixture lock is valid")
            .is_none()
            && std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(|path| path.join("python")))
                .is_none_or(|path| !path.is_dir())
        {
            // Non-Python test Nodes have no installation image. Explicit skill
            // and MCP Python requests still use the strict runtime entry point.
            return Ok(None);
        }
        let profile = self.profile.as_deref().ok_or_else(|| {
            Fault::new(ErrorCode::NotConfigured, "Python requires a Node profile")
        })?;
        self.python(environment)
            .await?
            .execution(profile, session, skills, stop)
            .await
            .map(Some)
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
            .ok_or_else(|| sailry_protocol::Fault::invalid("plugin.json is required"))?;
        let manifest = manifest::parse(&bytes)?;
        if manifest.name != name {
            return Err(sailry_protocol::Fault::invalid(
                "plugin name does not match the selected package",
            ));
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
            let bytes = package::read(&staged, "plugin.json", 64 * 1024)?.ok_or_else(|| {
                sailry_protocol::Fault::invalid("staged plugin manifest is unavailable")
            })?;
            let manifest = manifest::parse(&bytes)?;
            if manifest.name != name {
                return Err(sailry_protocol::Fault::invalid(
                    "plugin manifest changed during installation",
                ));
            }
            let info = describe(&staged, manifest, digest.clone(), tree.issues)?;
            if info.issues.iter().any(|issue| {
                matches!(
                    issue.kind,
                    sailry_protocol::plugin::IssueKind::InvalidSkill
                        | sailry_protocol::plugin::IssueKind::InvalidSkills
                )
            }) {
                return Err(sailry_protocol::Fault::invalid(
                    "plugin contains invalid skills",
                ));
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
                    crate::files::path::sync(&packages).map_err(io_error)?;
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

fn describe(
    source: &cap_std::fs::Dir,
    mut manifest: manifest::Manifest,
    digest: String,
    issues: Vec<sailry_protocol::plugin::Issue>,
) -> Result<Info, Fault> {
    manifest.issues.extend(issues);
    let skills = skills::discover(source, &mut manifest.issues);
    let servers = mcp::discover(source, &mut manifest.issues);
    if manifest.extension.as_ref().is_some_and(|extension| {
        extension.cards.iter().any(|card| {
            card.mcp_server.as_ref().is_some_and(|name| {
                servers.get(name).is_none_or(|server| {
                    server.transport() == sailry_protocol::plugin::McpTransport::Stdio
                })
            })
        })
    }) {
        manifest.issues.push(sailry_protocol::plugin::Issue {
            path: "plugin.json.extensions.dev.sailry.platform.cards".into(),
            kind: sailry_protocol::plugin::IssueKind::InvalidExtensions,
        });
        manifest.extension = None;
    }
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
        .map_err(|_| sailry_protocol::Fault::invalid("plugin metadata could not be encoded"))?
        .len()
        > 256 * 1024
    {
        return Err(sailry_protocol::Fault::invalid(
            "plugin metadata exceeds the component limit",
        ));
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
