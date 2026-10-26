//! Node-owned immutable packages. Behavioral references: Code 67ae9fa0
//! production/storage/packages.rs, production/mcp.rs and Platform 727ce0a harbor-agent-plugins
//! (Apache-2.0). No historical UI or parallel execution host is imported.
mod archive;
pub(crate) mod authorization;
mod bundled;
pub(crate) mod cards;
pub(crate) mod catalog;
mod collection;
pub(crate) mod compatibility;
pub(crate) mod connectors;
pub(crate) mod conversation;
pub(crate) mod github;
pub(crate) mod http;
pub(crate) mod icon;
mod manifest;
pub(crate) mod mcp;
pub(crate) mod modes;
mod notes;
pub(crate) mod options;
mod package;
mod query;
pub(crate) mod registry;
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

pub(crate) use bundled::{AUTHENTICATIONS, DEFAULTS};
pub(crate) use manifest::validate_name;

#[derive(Clone)]
pub(crate) struct Host {
    profile: Option<PathBuf>,
    pub(crate) authorizations: std::sync::Arc<authorization::Clients>,
    pub(crate) connectors: std::sync::Arc<connectors::Service>,
    github: github::Github,
    pub(crate) registry: registry::Registry,
    official_catalog: std::sync::Arc<catalog::official::Cache>,
    pub(crate) provider_reads: std::sync::Arc<http::provider::cache::Cache>,
    external_skill_catalog: std::sync::Arc<catalog::skills::Directory>,
    registry_reads: std::sync::Arc<registry::Cache>,
    python: std::sync::Arc<tokio::sync::OnceCell<std::sync::Arc<crate::python::Manager>>>,
    python_index: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    python_progress:
        std::sync::Arc<tokio::sync::watch::Sender<Vec<sailry_protocol::python::Preparation>>>,
    python_packages: std::sync::Arc<std::sync::Mutex<Vec<Info>>>,
    #[cfg(any(test, feature = "test-support"))]
    python_runtime: std::sync::Arc<std::sync::Mutex<Option<PathBuf>>>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) script_timeout: std::sync::Arc<std::sync::Mutex<Option<std::time::Duration>>>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) script_wait: std::sync::Arc<std::sync::Mutex<Option<script::Wait>>>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) flow_wait: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<FlowWait>>>,
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) struct FlowWait {
    pub(crate) tool: Option<String>,
    pub(crate) call: Option<String>,
    pub(crate) wait: script::Wait,
}

impl Host {
    pub(crate) fn profile(&self) -> Option<&std::path::Path> {
        self.profile.as_deref()
    }
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
            authorizations: Default::default(),
            connectors: Default::default(),
            github: Default::default(),
            registry: Default::default(),
            official_catalog: Default::default(),
            provider_reads: Default::default(),
            external_skill_catalog: Default::default(),
            registry_reads: Default::default(),
            python: Default::default(),
            python_index: Default::default(),
            python_progress: std::sync::Arc::new(tokio::sync::watch::channel(Vec::new()).0),
            python_packages: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            python_runtime: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            script_timeout: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            script_wait: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            flow_wait: Default::default(),
        }
    }

    pub(crate) fn script_deadline(&self) -> std::time::Instant {
        #[cfg(any(test, feature = "test-support"))]
        let timeout = *self
            .script_timeout
            .lock()
            .expect("script fixture lock is valid");
        #[cfg(not(any(test, feature = "test-support")))]
        let timeout = None;
        std::time::Instant::now() + timeout.unwrap_or(script::CALL_TIMEOUT)
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
        let bytes = package::read(&source, "plugin.json", manifest::MAX_BYTES)?
            .ok_or_else(|| sailry_protocol::Fault::invalid("plugin.json is required"))?;
        let manifest = manifest::parse(&bytes)?;
        if manifest.name != name {
            return Err(sailry_protocol::Fault::invalid(
                "plugin name does not match the selected package",
            ));
        }
        self.stage(name, |staged| {
            let mut tree = package::Tree::default();
            tree.walk(&source, Some(staged), "", 0)?;
            Ok(tree)
        })
    }

    fn stage(
        &self,
        name: &str,
        write: impl FnOnce(&cap_std::fs::Dir) -> Result<package::Tree, Fault>,
    ) -> Result<Info, Fault> {
        use cap_fs_ext::DirExt;
        let profile = self
            .profile
            .as_deref()
            .ok_or_else(|| Fault::new(ErrorCode::Unavailable, "plugin storage is unavailable"))?;
        let profile = crate::files::path::root(profile)?;
        let packages = directory(&directory(&profile, "plugins")?, "packages")?;
        let staging = cap_tempfile::TempDir::new_in(&packages).map_err(io_error)?;
        let result = (|| {
            staging.create_dir("package").map_err(io_error)?;
            let staged = staging.open_dir_nofollow("package").map_err(io_error)?;
            let tree = write(&staged)?;
            let digest = tree.digest();
            // Parse the copied bytes, not a path that may have changed during staging.
            let bytes =
                package::read(&staged, "plugin.json", manifest::MAX_BYTES)?.ok_or_else(|| {
                    sailry_protocol::Fault::invalid("staged plugin manifest is unavailable")
                })?;
            let manifest = manifest::parse(&bytes)?;
            if manifest.name != name {
                return Err(sailry_protocol::Fault::invalid(
                    "plugin manifest changed during installation",
                ));
            }
            let info = describe(&staged, manifest, digest.clone(), tree.issues)?;
            compatibility::check(&info)?;
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
    if manifest
        .extension
        .as_ref()
        .is_some_and(|extension| !connectors::validate(extension, &servers, settings.as_ref()))
    {
        manifest.issues.push(sailry_protocol::plugin::Issue {
            path: "plugin.json.extensions.dev.sailry.platform.connectors".into(),
            kind: sailry_protocol::plugin::IssueKind::InvalidExtensions,
        });
        manifest.extension = None;
    }
    let mcp = servers
        .iter()
        .map(|(name, server)| sailry_protocol::plugin::Mcp {
            name: name.clone(),
            transport: server.transport(),
            allowed: true,
            availability: Default::default(),
            host: match server {
                mcp::Server::Http { url, .. } | mcp::Server::Sse { url, .. } => {
                    url::Url::parse(url)
                        .ok()
                        .and_then(|url| url.host_str().map(str::to_owned))
                }
                mcp::Server::Stdio { .. } => None,
            },
        })
        .collect();
    let icon = icon::read(source, manifest.extension.as_ref(), &mut manifest.issues);
    let release_notes = match notes::read(manifest.version.as_deref(), |name| {
        package::read(source, name, sailry_protocol::MAX_FRAME_BYTES)
    }) {
        Ok(notes) => notes,
        Err(_) => {
            manifest.issues.push(sailry_protocol::plugin::Issue {
                path: "CHANGELOG.md".into(),
                kind: sailry_protocol::plugin::IssueKind::InvalidExtensions,
            });
            None
        }
    };
    let mut info = Info {
        versions: Vec::new(),
        release_notes,
        origin: None,
        icon,
        skill: None,
        mcp_source: None,
        extension: manifest.extension,
        summary: Summary {
            compatibility: Default::default(),
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
    compatibility::refresh(&mut info);
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
