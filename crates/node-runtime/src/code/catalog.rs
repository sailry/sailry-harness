//! Node-wide installation metadata, independent of project discovery and running instances.
use super::{Pool, install, resolve};
use sailry_protocol::{Fault, code};

impl Pool {
    pub(crate) async fn catalog(&self) -> Result<Vec<code::Language>, Fault> {
        let rules = self.rules.get()?;
        let settings = self.settings();
        let installations = self.installations()?;
        let environment = self.environment.clone();
        tokio::task::spawn_blocking(move || {
            let mut languages = Vec::new();
            for language in &rules.languages {
                let mut servers = Vec::new();
                for name in settings
                    .languages
                    .get(&language.name)
                    .unwrap_or(&language.servers)
                {
                    let base = rules.servers.get(name);
                    let custom = settings.servers.get(name);
                    let Some(command) = custom
                        .and_then(|value| value.command.as_ref())
                        .or_else(|| base.map(|server| &server.command))
                    else {
                        continue;
                    };
                    let package = rules
                        .package(command)
                        .or_else(|| base.and_then(|server| rules.package(&server.command)));
                    let args = custom
                        .and_then(|value| value.args.as_deref())
                        .or_else(|| base.map(|server| server.args.as_slice()))
                        .unwrap_or_default();
                    let mut variables = base
                        .map(|server| server.environment.clone())
                        .unwrap_or_default();
                    if let Some(overrides) = custom.and_then(|value| value.environment.as_ref()) {
                        variables.extend(overrides.clone());
                    }
                    let installed =
                        package.is_some_and(|package| {
                            installations.iter().any(|installation| {
                                installation.package == package.name
                                    && installation.receipt.is_some()
                            })
                        }) || resolve::global_executable(command, &environment, args, &variables);
                    servers.push(code::Candidate {
                        server: name.clone(),
                        command: command.clone(),
                        disabled: custom.is_some_and(|value| value.disabled),
                        status: if installed {
                            code::Status::Stopped
                        } else {
                            code::Status::NotInstalled
                        },
                        package: package.map(|package| install::package(package, &environment)),
                        startup: None,
                        startup_approved: false,
                    });
                }
                if !servers.is_empty() {
                    languages.push(code::Language {
                        name: language.name.clone(),
                        id: language.id.clone(),
                        servers,
                    });
                }
            }
            languages
        })
        .await
        .map_err(|error| Fault::invalid(error.to_string()))
    }
}
