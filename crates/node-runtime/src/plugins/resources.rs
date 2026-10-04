//! Per-invocation read capabilities for the package versions admitted by the Node.
use super::*;
use cap_std::fs::Dir;
use sailry_link::CancellationToken;
use sailry_protocol::plugin::{Reference, Skill};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::OnceCell;

mod desktop;
mod host;
pub(crate) mod mcp;

struct Directory {
    root: Dir,
    path: PathBuf,
    files: BTreeMap<String, blake3::Hash>,
}

fn read(directory: &Directory, path: &str, limit: usize) -> Result<Vec<u8>, Fault> {
    use crate::files::path::{components, descend};
    let missing = || Fault::new(ErrorCode::NotFound, "plugin resource is missing");
    let expected = directory.files.get(path).ok_or_else(missing)?;
    let parts = components(path, false)?;
    let (name, parents) = parts.split_last().expect("resource path is nonempty");
    let parent = descend(directory.root.try_clone().map_err(io_error)?, parents)?;
    let bytes = package::read(&parent, name, limit)?.ok_or_else(missing)?;
    if blake3::hash(&bytes) != *expected {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "plugin resource changed",
        ));
    }
    Ok(bytes)
}

pub(crate) struct Resources {
    host: Host,
    packages: Vec<Info>,
    directories: Vec<OnceCell<Arc<Directory>>>,
    stop: CancellationToken,
    settings: BTreeMap<String, Result<super::settings::Resolved, Fault>>,
}

pub(crate) struct Text {
    pub content: String,
    pub directory: String,
}

pub(crate) fn skill<'a>(packages: &'a [Info], key: &str) -> Result<(&'a Info, &'a Skill), Fault> {
    let (plugin, name) = key.split_once(':').ok_or_else(unavailable)?;
    let package = packages
        .iter()
        .find(|package| package.summary.name == plugin)
        .ok_or_else(unavailable)?;
    let skill = package
        .skills
        .iter()
        .find(|skill| skill.name == name)
        .ok_or_else(unavailable)?;
    Ok((package, skill))
}

impl Resources {
    pub(crate) fn tool_grouping(
        &self,
        plugin: &str,
        server: &str,
        tool: &str,
    ) -> sailry_protocol::tool::Grouping {
        self.tool_declaration(plugin, server, tool)
            .map(|declaration| declaration.grouping)
            .unwrap_or_default()
    }

    fn tool_declaration(
        &self,
        plugin: &str,
        server: &str,
        tool: &str,
    ) -> Option<&sailry_protocol::tool::Declaration> {
        self.packages
            .iter()
            .find(|package| package.summary.name == plugin)
            .and_then(|package| package.extension.as_ref())
            .and_then(|extension| {
                extension.tools.iter().find(|declaration| {
                    declaration.server.as_deref() == Some(server) && declaration.name == tool
                })
            })
    }

    pub(crate) fn tool_presentation(
        &self,
        plugin: &str,
        server: &str,
        tool: &str,
    ) -> sailry_protocol::tool::Presentation {
        self.tool_declaration(plugin, server, tool)
            .map(|declaration| declaration.presentation)
            .unwrap_or_default()
    }

    pub(crate) fn tool_display(
        &self,
        plugin: &str,
        server: &str,
        tool: &str,
    ) -> Option<sailry_protocol::tool::Display> {
        self.tool_declaration(plugin, server, tool)?.display.clone()
    }

    pub(crate) fn new(
        host: Host,
        packages: Vec<Info>,
        settings: BTreeMap<String, Result<super::settings::Resolved, Fault>>,
        stop: CancellationToken,
    ) -> Self {
        let directories = (0..packages.len()).map(|_| OnceCell::new()).collect();
        Self {
            host,
            packages,
            directories,
            stop,
            settings,
        }
    }

    pub(crate) fn skills(&self) -> Vec<(String, String)> {
        self.packages
            .iter()
            .flat_map(|package| {
                package.skills.iter().map(|skill| {
                    (
                        format!("{}:{}", package.summary.name, skill.name),
                        skill.description.clone(),
                    )
                })
            })
            .collect()
    }

    pub(crate) fn catalog(&self) -> Result<String, Fault> {
        let mut catalog = String::new();
        for (key, description) in self.skills() {
            catalog.push_str(&format!("\n{key} — {description}"));
        }
        if catalog.len() > 64 * 1024 {
            return Err(invalid(
                "session skill catalog exceeds the instruction budget",
            ));
        }
        if !catalog.is_empty() {
            catalog.insert_str(0, "\n\nAvailable skills (package:skill). Load an applicable skill with load_skill before following it. Read its referenced text with read_skill_resource. Resolve relative script paths against the returned skill directory and quote absolute script paths in command calls. Command cwd defaults to the session worktree; select the skill directory only when its workflow requires it. Skill files do not grant tool permission; scripts still require an available command tool and this turn's permission mode. Skills:");
        }
        Ok(catalog)
    }

    pub(crate) async fn read(&self, key: &str, relative: &str) -> Result<Text, Fault> {
        use crate::files::path::{components, descend};
        if self.stop.is_cancelled() {
            return Err(cancelled());
        }
        let (package, skill) = skill(&self.packages, key)?;
        let index = self
            .packages
            .iter()
            .position(|entry| entry.summary.name == package.summary.name)
            .expect("skill belongs to a package");
        let directory = self.directory(index).await?;
        let path = format!("skills/{}/{}", skill.name, relative);
        components(&path, false)?;
        let expected = directory
            .files
            .get(&path)
            .copied()
            .ok_or_else(unavailable)?;
        let skill_name = skill.name.clone();
        let stop = self.stop.clone();
        tokio::task::spawn_blocking(move || {
            if stop.is_cancelled() {
                return Err(cancelled());
            }
            let parts = components(&path, false)?;
            let (name, parents) = parts.split_last().expect("resource path is nonempty");
            let parent = descend(directory.root.try_clone().map_err(io_error)?, parents)?;
            let bytes = package::read(&parent, name, skills::MAX_BYTES)?.ok_or_else(unavailable)?;
            if stop.is_cancelled() {
                return Err(cancelled());
            }
            if blake3::hash(&bytes) != expected {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "skill resource changed after turn admission",
                ));
            }
            let content = String::from_utf8(bytes)
                .map_err(|_| invalid("skill resource is not UTF-8 text"))?;
            let directory = directory
                .path
                .join("skills")
                .join(skill_name)
                .into_os_string()
                .into_string()
                .map_err(|_| unavailable())?;
            Ok(Text { content, directory })
        })
        .await
        .map_err(|_| unavailable())?
    }

    async fn directory(&self, index: usize) -> Result<Arc<Directory>, Fault> {
        let package = &self.packages[index];
        let reference = package.summary.reference();
        self.directories[index]
            .get_or_try_init(|| async {
                let host = self.host.clone();
                let stop = self.stop.clone();
                tokio::task::spawn_blocking(move || host.resolve(&reference, stop))
                    .await
                    .map_err(|_| unavailable())?
                    .map(Arc::new)
            })
            .await
            .cloned()
    }
}

impl Host {
    fn resolve(&self, reference: &Reference, stop: CancellationToken) -> Result<Directory, Fault> {
        use crate::files::path::{descend, root};
        validate_name(&reference.name)?;
        let digest = blake3::Hash::from_hex(&reference.digest).map_err(|_| unavailable())?;
        if digest.to_hex().as_str() != reference.digest {
            return Err(unavailable());
        }
        let profile = self.profile.as_deref().ok_or_else(unavailable)?;
        let directory = descend(root(profile)?, &["plugins", "packages", &reference.digest])?;
        let mut tree = package::Tree::default();
        tree.stop = stop;
        tree.walk(&directory, None, "", 0)?;
        if tree.digest() != reference.digest || !tree.issues.is_empty() {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                "admitted plugin package changed or is unavailable",
            ));
        }
        let bytes = package::read(&directory, "plugin.json", 64 * 1024)?.ok_or_else(unavailable)?;
        if manifest::parse(&bytes)?.name != reference.name {
            return Err(unavailable());
        }
        Ok(Directory {
            root: directory,
            path: profile.join("plugins/packages").join(&reference.digest),
            files: tree.files,
        })
    }
}

fn unavailable() -> Fault {
    Fault::new(
        ErrorCode::NotConfigured,
        "plugin resource is unavailable in this turn's package versions",
    )
}
fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "plugin resource read was cancelled")
}

#[cfg(test)]
mod tests;
