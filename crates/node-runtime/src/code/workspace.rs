use super::{Pool, install, resolve, rules};
use sailry_link::CancellationToken;
use sailry_protocol::{Fault, WorktreeId, code};
use std::{collections::BTreeMap, path::PathBuf};

impl Pool {
    pub(crate) async fn packages(
        &self,
        worktree: WorktreeId,
        root: PathBuf,
        stop: &CancellationToken,
    ) -> Result<code::Workspace, Fault> {
        let rules = self.rules.get()?;
        let discovery = rules.clone();
        let directory = root.clone();
        let cancelled = stop.clone();
        let (paths, truncated) =
            tokio::task::spawn_blocking(move || scan(&directory, &discovery, cancelled))
                .await
                .map_err(|error| Fault::invalid(error.to_string()))??;
        let settings = self
            .settings
            .read()
            .expect("code settings lock is healthy")
            .clone();
        let mut languages = Vec::new();
        for (name, path) in paths {
            let Some(language) = rules
                .languages
                .iter()
                .find(|language| language.name == name)
            else {
                continue;
            };
            let captured_root = root.clone();
            let profile = self.profile.clone();
            let environment = self.environment.clone();
            let captured_path = path.clone();
            let captured_settings = settings.clone();
            let resolver = self.resolver(rules.clone());
            let resolved = tokio::task::spawn_blocking(move || {
                resolver.resolve(
                    worktree,
                    &captured_root,
                    profile.as_deref(),
                    &environment,
                    &captured_path,
                    &captured_settings,
                )
            })
            .await
            .map_err(|error| Fault::invalid(error.to_string()))??;
            let _ = self
                .prepare(
                    worktree,
                    root.clone(),
                    &code::Query::Symbols {
                        path: Some(path.clone()),
                        query: None,
                    },
                )
                .await?;
            let running = self.list(worktree).await?;
            let mut servers = Vec::new();
            for name in settings
                .languages
                .get(&language.name)
                .unwrap_or(&language.servers)
            {
                let custom = settings.servers.get(name);
                let base = rules.servers.get(name);
                let command = custom
                    .and_then(|custom| custom.command.as_deref())
                    .or_else(|| base.map(|server| server.command.as_str()));
                let Some(command) = command else {
                    continue;
                };
                let package = rules
                    .package(command)
                    .map(|package| install::package(package, &self.environment));
                let startup = resolved
                    .plan
                    .as_ref()
                    .filter(|plan| plan.startup.server == *name)
                    .map(|plan| plan.startup.clone());
                let installed = base.is_some_and(|base| {
                    let mut server = base.clone();
                    server.command = command.into();
                    resolve::executable(
                        &root,
                        &root,
                        self.profile.as_deref(),
                        &self.environment,
                        &server,
                        rules.package(command),
                    )
                    .is_some()
                });
                let status = running
                    .iter()
                    .find(|server| {
                        server.startup.server == *name
                            && startup
                                .as_ref()
                                .is_none_or(|startup| startup == &server.startup)
                    })
                    .map(|server| server.status)
                    .unwrap_or(if startup.is_some() || installed {
                        code::Status::Stopped
                    } else {
                        code::Status::NotInstalled
                    });
                servers.push(code::Candidate {
                    server: name.clone(),
                    command: command.into(),
                    disabled: custom.is_some_and(|custom| custom.disabled),
                    status,
                    package,
                    startup,
                    startup_approved: false,
                });
            }
            languages.push(code::Language {
                name: language.name.clone(),
                id: language.id.clone(),
                servers,
            });
        }
        Ok(code::Workspace {
            worktree,
            languages,
            truncated,
        })
    }
}

fn scan(
    root: &std::path::Path,
    rules: &rules::Rules,
    cancelled: CancellationToken,
) -> Result<(BTreeMap<String, String>, bool), Fault> {
    use std::io::Read;
    let control = crate::files::Control::new(cancelled);
    let mut paths = BTreeMap::new();
    let report = crate::files::walk::visit(root, &control, &mut |directory, name, path| {
        let direct = rules.language(path, "");
        let mut text = String::new();
        if direct.is_none() {
            let mut bytes = Vec::new();
            if let Ok(file) = crate::files::open_regular(directory, name) {
                file.take(4096)
                    .read_to_end(&mut bytes)
                    .map_err(crate::files::io_error)?;
                text = String::from_utf8_lossy(&bytes).into_owned();
            }
        }
        if let Some(language) = direct.or_else(|| rules.language(path, &text)) {
            if paths.len() == 64 && !paths.contains_key(&language.name) {
                return Ok(false);
            }
            paths
                .entry(language.name.clone())
                .or_insert_with(|| path.into());
        }
        Ok(true)
    })?;
    Ok((paths, report.truncated))
}
