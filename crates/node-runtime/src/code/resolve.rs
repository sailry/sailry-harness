use super::{documents, identity, rules};
use sailry_protocol::{Fault, WorktreeId, code::Startup};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

#[derive(Clone, Serialize)]
pub(super) struct Plan {
    pub startup: Startup,
    pub root: PathBuf,
    pub profile: Option<PathBuf>,
    pub dependency: PathBuf,
    pub directory: PathBuf,
    pub executable: PathBuf,
    pub executable_revision: String,
    pub args: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub config: Value,
    pub timeout: Duration,
    pub language: String,
    #[serde(skip)]
    pub rules: std::sync::Arc<rules::Rules>,
}

pub(super) struct Executable {
    pub path: PathBuf,
    pub dependency: PathBuf,
}

pub(super) struct Resolved {
    pub plan: Option<Plan>,
    pub package: Option<String>,
}

pub(super) struct Resolver {
    pub rules: Arc<rules::Rules>,
    pub identities: Arc<identity::Cache>,
}

#[cfg(test)]
pub(super) fn resolve(
    worktree: WorktreeId,
    root: &Path,
    profile: Option<&Path>,
    environment: &crate::process::Environment,
    path: &str,
) -> Result<Resolved, Fault> {
    configured(
        worktree,
        root,
        profile,
        environment,
        path,
        &sailry_protocol::code::Settings::default(),
    )
}

#[cfg(test)]
pub(super) fn configured(
    worktree: WorktreeId,
    root: &Path,
    profile: Option<&Path>,
    environment: &crate::process::Environment,
    path: &str,
    settings: &sailry_protocol::code::Settings,
) -> Result<Resolved, Fault> {
    Resolver {
        rules: rules::Rules::load()?,
        identities: Arc::default(),
    }
    .resolve(worktree, root, profile, environment, path, settings)
}

impl Resolver {
    pub fn resolve(
        &self,
        worktree: WorktreeId,
        root: &Path,
        profile: Option<&Path>,
        environment: &crate::process::Environment,
        path: &str,
        settings: &sailry_protocol::code::Settings,
    ) -> Result<Resolved, Fault> {
        let rules = &self.rules;
        let text = documents::read(root, path)?;
        let Some(language) = rules.language(path, &text.text) else {
            return Ok(Resolved {
                plan: None,
                package: None,
            });
        };
        let directory = nearest(root, path, &language.roots)?;
        let mut suggested = None;
        for name in settings
            .languages
            .get(&language.name)
            .unwrap_or(&language.servers)
        {
            let override_ = settings.servers.get(name);
            if override_.is_some_and(|value| value.disabled) {
                continue;
            }
            let base = rules.servers.get(name).cloned();
            let mut server = if let Some(base) = base {
                base
            } else if let Some(command) = override_.and_then(|value| value.command.clone()) {
                rules::Server {
                    command,
                    args: Vec::new(),
                    config: Value::Null,
                    environment: BTreeMap::new(),
                    timeout: 20,
                    roots: Vec::new(),
                }
            } else {
                continue;
            };
            if let Some(value) = override_ {
                if let Some(command) = &value.command {
                    server.command = command.clone();
                }
                if let Some(args) = &value.args {
                    server.args = args.clone();
                }
                if let Some(config) = &value.config {
                    server.config = config.clone();
                }
                if let Some(environment) = &value.environment {
                    server.environment.extend(environment.clone());
                }
            }
            if !server.roots.is_empty() && !matches_root(&directory, &server.roots)? {
                continue;
            }
            let package = rules.package(&server.command);
            if suggested.is_none() {
                suggested = package.map(|package| package.name.clone());
            }
            let Some(executable) = override_
                .and_then(|value| value.command.as_ref())
                .and_then(|command| which::which_in(command, environment.path(), &directory).ok())
                .and_then(installed)
                .and_then(|path| selected(path, None))
                .or_else(|| executable(root, &directory, profile, environment, &server, package))
            else {
                continue;
            };
            let executable_revision = self.identities.revision(&executable.path)?;
            let relative = directory
                .strip_prefix(root)
                .map_err(|_| Fault::invalid("language root is outside the worktree"))?
                .to_string_lossy()
                .replace('\\', "/");
            let mut plan = Plan {
                startup: Startup {
                    worktree,
                    root: relative,
                    server: name.clone(),
                    executable: executable.path.to_string_lossy().into_owned(),
                    revision: String::new(),
                },
                root: root.to_owned(),
                profile: profile.map(Path::to_owned),
                dependency: executable.dependency,
                directory,
                executable: executable.path,
                executable_revision: executable_revision.clone(),
                args: server.args.clone(),
                environment: server.environment.clone(),
                config: server.config.clone(),
                timeout: Duration::from_secs(server.timeout),
                language: if language.id.is_empty() {
                    language.name.clone()
                } else {
                    language.id.clone()
                },
                rules: rules.clone(),
            };
            let mut hash = Sha256::new();
            let identity = serde_json::json!({
                "startup": plan.startup,
                "root": plan.root,
                "profile": plan.profile,
                "sandbox": {"supported":crate::process::sandbox::SUPPORTED,"access":"workspace","network":false,"dependency":plan.dependency},
                "directory": plan.directory,
                "executable": plan.executable,
                "args": plan.args,
                "environment": plan.environment,
                "config": plan.config,
                "timeout_ms": plan.timeout.as_millis(),
            });
            hash.update(
                serde_json::to_vec(&identity).map_err(|error| Fault::invalid(error.to_string()))?,
            );
            hash.update(executable_revision);
            // PATH resolution and the allowlisted login environment are frozen by the Node.
            hash.update(environment.identity());
            plan.startup.revision = format!("{:x}", hash.finalize());
            return Ok(Resolved {
                plan: Some(plan),
                package: suggested,
            });
        }
        Ok(Resolved {
            plan: None,
            package: suggested,
        })
    }
}

pub(super) fn executable(
    root: &Path,
    directory: &Path,
    profile: Option<&Path>,
    environment: &crate::process::Environment,
    server: &rules::Server,
    package: Option<&rules::Package>,
) -> Option<Executable> {
    if let (Some(profile), Some(package)) = (profile, package) {
        let preferred = super::install::version(package).ok();
        let versions = profile.canonicalize().ok().and_then(|profile| {
            super::install::versions(&profile, &package.name, preferred.as_deref()).ok()
        });
        for (version, _) in versions.into_iter().flatten() {
            for candidate in [
                version.join("bin").join(&server.command),
                version.join(&server.command),
            ] {
                if let Some(path) =
                    installed(candidate).and_then(|path| selected(path, Some(&version)))
                {
                    return Some(path);
                }
            }
        }
        let source = package
            .source
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("");
        if source.starts_with("pkg:npm/") || source.starts_with("pkg:pypi/") {
            let mut parent = Some(directory);
            while let Some(path) = parent {
                let base = if source.starts_with("pkg:npm/") {
                    path.join("node_modules/.bin")
                } else {
                    path.join(if cfg!(windows) {
                        ".venv/Scripts"
                    } else {
                        ".venv/bin"
                    })
                };
                if let Some(candidate) = installed(base.join(&server.command))
                    .filter(|candidate| candidate.starts_with(root))
                {
                    return selected(candidate, None);
                }
                if path == root {
                    break;
                }
                parent = path.parent().filter(|path| path.starts_with(root));
            }
        }
    }
    which::which_in(&server.command, environment.path(), directory)
        .ok()
        .and_then(installed)
        .and_then(|path| selected(path, None))
}

// A global catalog must not resolve relative commands or PATH entries against a project.
pub(super) fn global_executable(command: &str, environment: &crate::process::Environment) -> bool {
    let command = Path::new(command);
    if command.is_absolute() {
        return installed(command.to_owned()).is_some();
    }
    if command.components().count() != 1 {
        return false;
    }
    environment.path().is_some_and(|path| {
        std::env::split_paths(path)
            .filter(|directory| directory.is_absolute())
            .any(|directory| installed(directory.join(command)).is_some())
    })
}

fn selected(path: PathBuf, directory: Option<&Path>) -> Option<Executable> {
    let resolved = path.canonicalize().ok()?;
    let dependency = match directory {
        Some(directory) => {
            let canonical = directory.canonicalize().ok()?;
            if canonical != directory || !resolved.starts_with(&canonical) {
                return None;
            }
            canonical
        }
        None => resolved,
    };
    Some(Executable { path, dependency })
}

fn installed(path: PathBuf) -> Option<PathBuf> {
    #[cfg(windows)]
    let path = if path.is_file() {
        path
    } else {
        ["exe", "cmd", "bat"]
            .into_iter()
            .map(|extension| path.with_extension(extension))
            .find(|candidate| candidate.is_file())?
    };
    if !path.is_file() {
        return None;
    }
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.metadata().ok()?.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    Some(path)
}

fn nearest(root: &Path, path: &str, markers: &[String]) -> Result<PathBuf, Fault> {
    let mut directory = root
        .join(path)
        .parent()
        .ok_or_else(|| Fault::invalid("code document has no parent"))?
        .to_owned();
    loop {
        let relative = directory
            .strip_prefix(root)
            .map_err(|_| Fault::invalid("language root escaped the worktree"))?
            .to_string_lossy()
            .replace('\\', "/");
        documents::directory(root, &relative)?;
        if directory == root || matches_root(&directory, markers)? {
            return Ok(directory);
        }
        if !directory.pop() {
            return Err(Fault::invalid("language root is unavailable"));
        }
    }
}

fn matches_root(directory: &Path, markers: &[String]) -> Result<bool, Fault> {
    let names = std::fs::read_dir(directory)
        .map_err(|error| Fault::invalid(format!("language root cannot be read: {error}")))?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .collect::<Vec<_>>();
    Ok(markers
        .iter()
        .filter_map(|pattern| globset::Glob::new(pattern).ok())
        .any(|glob| {
            let matcher = glob.compile_matcher();
            names.iter().any(|name| matcher.is_match(Path::new(name)))
        }))
}
