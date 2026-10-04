//! Resolve MCP launch inputs only in the execution Node's frozen package version.
use super::*;
use crate::plugins::mcp::{self, Server};
mod projection;
pub(crate) use projection::Projection;

pub(crate) struct Configured {
    pub plugin: String,
    pub reference: sailry_protocol::plugin::Reference,
    pub name: String,
    pub launch: Result<Launch, Fault>,
}

pub(crate) enum Launch {
    Stdio {
        command: PathBuf,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        cwd: PathBuf,
        // Keep resource roots alive until the managed connection is closed.
        roots: (Dir, Dir),
        settings: Option<Projection>,
    },
    Http {
        url: String,
        headers: BTreeMap<String, String>,
        sse: bool,
        authorization: Option<sailry_protocol::CredentialId>,
    },
}

impl Resources {
    pub(crate) async fn mcp_client(
        &self,
        ingress: &Arc<crate::store::Ingress>,
        plugin: &str,
        server: &str,
        launch: &Launch,
    ) -> Result<Option<rmcp::transport::auth::AuthClient<reqwest::Client>>, Fault> {
        let Launch::Http {
            url,
            headers,
            authorization: Some(id),
            ..
        } = launch
        else {
            return Ok(None);
        };
        if headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("authorization") && !value.is_empty())
        {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "MCP OAuth conflicts with a configured authorization header",
            ));
        }
        let target = crate::plugins::authorization::Target {
            plugin: plugin.into(),
            server: server.into(),
            endpoint: url.clone(),
            id: *id,
        };
        if ingress.mcp_credentials(target.clone()).await?.is_none() {
            return Err(Fault::new(
                ErrorCode::NotConfigured,
                "MCP authorization is unavailable",
            ));
        }
        self.host
            .authorizations
            .client(ingress, target)
            .await
            .map(Some)
    }

    pub(crate) async fn mcp_endpoint(&self, plugin: &str, server: &str) -> Result<String, Fault> {
        let index = self
            .packages
            .iter()
            .position(|package| package.summary.name == plugin)
            .ok_or_else(unavailable)?;
        let directory = self.directory(index).await?;
        let configured = self
            .settings
            .get(plugin)
            .cloned()
            .transpose()?
            .and_then(|settings| settings.mcp);
        let server = server.to_owned();
        tokio::task::spawn_blocking(move || {
            let mut declared = servers(&directory)?;
            if let Some(configured) = configured {
                mcp::validate_configuration(&declared, &configured)?;
                declared = configured.servers;
            }
            match declared.remove(&server).ok_or_else(unavailable)? {
                Server::Http { url, .. } | Server::Sse { url, .. } => Ok(url),
                Server::Stdio { .. } => Err(unavailable()),
            }
        })
        .await
        .map_err(|_| unavailable())?
    }

    pub(crate) async fn mcp(&self) -> Vec<Configured> {
        let mut configured = Vec::new();
        for (index, package) in self.packages.iter().enumerate() {
            if package.mcp.is_empty() {
                continue;
            }
            let result = async {
                let directory = self.directory(index).await?;
                let host = self.host.clone();
                let name = package.summary.name.clone();
                let schema = package.settings.clone();
                let settings = self.settings.get(&name).cloned().unwrap_or_else(|| {
                    if package
                        .extension
                        .as_ref()
                        .is_some_and(|extension| extension.settings_schema.is_some())
                    {
                        Err(unavailable())
                    } else {
                        Ok(crate::plugins::settings::Resolved {
                            values: BTreeMap::new(),
                            secrets: BTreeMap::new(),
                            authorizations: BTreeMap::new(),
                            mcp: None,
                        })
                    }
                })?;
                let stop = self.stop.clone();
                tokio::task::spawn_blocking(move || {
                    if stop.is_cancelled() {
                        return Err(cancelled());
                    }
                    let mut servers = servers(&directory)?;
                    if let Some(configuration) = &settings.mcp {
                        mcp::validate_configuration(&servers, configuration)?;
                        servers = configuration.servers.clone();
                    }
                    Ok::<_, Fault>(
                        servers
                            .into_iter()
                            .map(|(server, config)| {
                                let launch = prepare(
                                    &host,
                                    &directory,
                                    &name,
                                    &server,
                                    config,
                                    schema.as_ref(),
                                    &settings,
                                );
                                (server, launch)
                            })
                            .collect::<BTreeMap<_, _>>(),
                    )
                })
                .await
                .map_err(|_| unavailable())?
            }
            .await;
            let mut servers = result.unwrap_or_default();
            for server in &package.mcp {
                configured.push(Configured {
                    plugin: package.summary.name.clone(),
                    reference: package.summary.reference(),
                    name: server.name.clone(),
                    launch: servers
                        .remove(&server.name)
                        .unwrap_or_else(|| Err(unavailable())),
                });
            }
        }
        configured
    }
}

fn servers(directory: &Directory) -> Result<BTreeMap<String, Server>, Fault> {
    let bytes =
        package::read(&directory.root, "mcp.json", mcp::MAX_BYTES)?.ok_or_else(unavailable)?;
    if directory.files.get("mcp.json") != Some(&blake3::hash(&bytes)) {
        return Err(unavailable());
    }
    mcp::parse(&bytes, &mut Vec::new())
}

impl Host {
    pub(crate) async fn read_mcp(
        &self,
        reference: sailry_protocol::plugin::Reference,
        stop: sailry_link::CancellationToken,
    ) -> Result<sailry_protocol::plugin::mcp::Configuration, Fault> {
        let permit = self
            .reads
            .clone()
            .try_acquire_owned()
            .map_err(|_| Fault::new(ErrorCode::Busy, "plugin resource workers are busy"))?;
        let closed = stop;
        let stop = closed.child_token();
        let _guard = stop.clone().drop_guard();
        let host = self.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            host.mcp_configuration(&reference, stop)
        });
        tokio::select! {
            _ = closed.cancelled() => Err(cancelled()),
            result = tokio::time::timeout(std::time::Duration::from_secs(5), worker) => result
                .map_err(|_| Fault::new(ErrorCode::Busy, "plugin resource read deadline exceeded"))?
                .map_err(|_| Fault::new(ErrorCode::Internal, "plugin resource worker failed"))?,
        }
    }

    pub(crate) fn mcp_configuration(
        &self,
        reference: &sailry_protocol::plugin::Reference,
        stop: sailry_link::CancellationToken,
    ) -> Result<sailry_protocol::plugin::mcp::Configuration, Fault> {
        let directory = self.resolve(reference, stop)?;
        Ok(sailry_protocol::plugin::mcp::Configuration {
            schema: mcp::SCHEMA.into(),
            servers: servers(&directory)?,
        })
    }

    pub(crate) fn validate_mcp(
        &self,
        reference: &sailry_protocol::plugin::Reference,
        configuration: &sailry_protocol::plugin::mcp::Configuration,
    ) -> Result<(), Fault> {
        let declared = self.mcp_configuration(reference, sailry_link::CancellationToken::new())?;
        mcp::validate_configuration(&declared.servers, configuration)
    }
}

fn prepare(
    host: &Host,
    package: &Directory,
    name: &str,
    server_name: &str,
    server: Server,
    schema: Option<&sailry_protocol::plugin::settings::Schema>,
    settings: &crate::plugins::settings::Resolved,
) -> Result<Launch, Fault> {
    let sse = matches!(&server, Server::Sse { .. });
    match server {
        Server::Http { url, mut headers } | Server::Sse { url, mut headers } => {
            for (field, secret) in &settings.secrets {
                let binding = schema
                    .and_then(|schema| schema.properties.get(field))
                    .and_then(|field| field.secret.as_ref())
                    .ok_or_else(unavailable)?;
                if binding.server.as_deref() == Some(server_name)
                    && let Some(header) = &binding.header
                {
                    let value = headers
                        .iter_mut()
                        .find(|(name, _)| name.eq_ignore_ascii_case(header))
                        .map(|(_, value)| value)
                        .ok_or_else(unavailable)?;
                    *value = secret.expose().into();
                }
            }
            Ok(Launch::Http {
                url,
                headers,
                sse,
                authorization: settings.authorizations.get(server_name).copied(),
            })
        }
        Server::Stdio {
            command,
            args,
            mut env,
            cwd,
        } => {
            let profile = host.profile.as_ref().ok_or_else(unavailable)?;
            let data = directory(
                &directory(
                    &directory(&crate::files::path::root(profile)?, "plugins")?,
                    "data",
                )?,
                name,
            )?;
            let data_path = profile.join("plugins/data").join(name);
            let root_text = package.path.to_str().ok_or_else(unavailable)?;
            let data_text = data_path.to_str().ok_or_else(unavailable)?;
            let (in_data, _) = mcp::cwd_root(&cwd).ok_or_else(unavailable)?;
            let expanded = mcp::expand(&cwd, root_text, data_text);
            let (base, base_path) = if in_data {
                (&data, &data_path)
            } else {
                (&package.root, &package.path)
            };
            let relative = if cwd.starts_with("./") {
                Path::new(&expanded)
                    .strip_prefix(".")
                    .map_err(|_| unavailable())?
            } else {
                Path::new(&expanded)
                    .strip_prefix(base_path)
                    .map_err(|_| unavailable())?
            };
            let relative = if relative.as_os_str().is_empty() {
                Path::new(".")
            } else {
                relative
            };
            let resolved = base.canonicalize(relative).map_err(io_error)?;
            let _cwd = base.open_dir(&resolved).map_err(io_error)?;
            let cwd = base_path.join(resolved);
            let command = if let Some(relative) = command.strip_prefix("./") {
                let resolved = package.root.canonicalize(relative).map_err(io_error)?;
                let parts = crate::files::path::components(
                    resolved.to_str().ok_or_else(unavailable)?,
                    false,
                )?;
                let (file, parents) = parts.split_last().ok_or_else(unavailable)?;
                let parent = crate::files::path::descend(
                    package.root.try_clone().map_err(io_error)?,
                    parents,
                )?;
                crate::files::open_regular(&parent, file)?;
                package.path.join(resolved)
            } else {
                PathBuf::from(command)
            };
            let args = args
                .iter()
                .map(|arg| mcp::expand(arg, root_text, data_text))
                .collect();
            for value in env.values_mut() {
                *value = mcp::expand(value, root_text, data_text);
            }
            env.insert("PLUGIN_ROOT".into(), root_text.into());
            env.insert("PLUGIN_DATA".into(), data_text.into());
            // Public projection and secret slots are execution inputs, never expanded templates.
            let settings_file = if schema.is_some() {
                let file = Projection::create(&data, &settings.values)?;
                env.insert(
                    "SAILRY_PLUGIN_SETTINGS_FILE".into(),
                    data_path
                        .join(file.name())
                        .to_str()
                        .ok_or_else(unavailable)?
                        .into(),
                );
                Some(file)
            } else {
                None
            };
            for (field, secret) in &settings.secrets {
                let binding = schema
                    .and_then(|schema| schema.properties.get(field))
                    .and_then(|field| field.secret.as_ref())
                    .ok_or_else(unavailable)?;
                if binding.server.as_deref() == Some(server_name)
                    && let Some(key) = &binding.env
                {
                    *env.get_mut(key).ok_or_else(unavailable)? = secret.expose().into();
                }
            }
            Ok(Launch::Stdio {
                command,
                args,
                env,
                cwd,
                roots: (package.root.try_clone().map_err(io_error)?, data),
                settings: settings_file,
            })
        }
    }
}

#[cfg(test)]
mod tests;
