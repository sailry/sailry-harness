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
    pub options: sailry_protocol::plugin::mcp::Options,
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
        python: Option<crate::python::Ready>,
    },
    Http {
        url: String,
        headers: BTreeMap<String, String>,
        sse: bool,
    },
}

impl Resources {
    pub(crate) async fn mcp_authorization(
        &self,
        ingress: &Arc<crate::store::Ingress>,
        context: sailry_protocol::plugin::Context,
        server: &str,
        launch: &mut Launch,
    ) -> Result<Option<crate::plugins::connectors::mcp::Authorization>, Fault> {
        let targets = ingress
            .mcp_connectors(context.clone(), server.into())
            .await?;
        let mut authorization = crate::plugins::connectors::mcp::Authorization::new(
            ingress.clone(),
            targets,
            self.stop.clone(),
        );
        if let Some(settings) = self
            .settings
            .get(&context.package.name)
            .cloned()
            .transpose()?
        {
            if !settings.secrets.is_empty() {
                authorization.settings = Some(Arc::new((context.clone(), server.into())));
            }
            for secret in settings.secrets.values() {
                authorization.remember(secret.clone())?;
            }
        }
        authorization.oauth = self
            .mcp_client(
                ingress,
                &context.package.name,
                server,
                launch,
                Some(&authorization),
            )
            .await?;
        if let Launch::Stdio { env, .. } = launch {
            for (target, injection) in &authorization.targets {
                let name = injection.env.as_deref().ok_or_else(unavailable)?;
                let credential = target.credential(injection)?;
                let Some(access) = ingress
                    .plugins
                    .connectors
                    .access_for(target, &credential, ingress, None, &self.stop)
                    .await?
                else {
                    continue;
                };
                let secret = access.token;
                env.insert(name.into(), secret.expose().into());
                authorization.seal(target.id, credential, secret.clone())?;
                authorization.remember(secret)?;
            }
        }
        if authorization.targets.is_empty()
            && self
                .settings
                .get(&context.package.name)
                .and_then(|settings| settings.as_ref().ok())
                .is_none_or(|settings| settings.secrets.is_empty())
        {
            Ok(None)
        } else {
            Ok(Some(authorization))
        }
    }
    async fn mcp_client(
        &self,
        ingress: &Arc<crate::store::Ingress>,
        plugin: &str,
        server: &str,
        launch: &Launch,
        authorization: Option<&crate::plugins::connectors::mcp::Authorization>,
    ) -> Result<Option<rmcp::transport::auth::AuthClient<reqwest::Client>>, Fault> {
        let Some(authorization) = authorization else {
            return Ok(None);
        };
        let Some((connector, _)) = authorization.targets.iter().find(|(target, _)| {
            target.method == sailry_protocol::plugin::connector::Kind::McpOauth
        }) else {
            return Ok(None);
        };
        let Launch::Http { url, headers, .. } = launch else {
            return Err(unavailable());
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
        let registered = crate::plugins::authorization::Registered::connector(connector)?;
        let target = crate::plugins::authorization::Target {
            plugin: plugin.into(),
            server: server.into(),
            endpoint: url.clone(),
            id: connector.id,
            revision: connector.revision,
            checking: false,
        };
        if ingress.mcp_credentials(target.clone()).await?.is_none() {
            return Err(unavailable());
        }
        self.host
            .authorizations
            .client(ingress, target, registered)
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

    pub(crate) async fn mcp(&self, environment: &crate::process::Environment) -> Vec<Configured> {
        self.configured(environment, crate::computer::helper::executable)
            .await
    }

    async fn configured(
        &self,
        environment: &crate::process::Environment,
        helper: fn() -> Result<PathBuf, Fault>,
    ) -> Vec<Configured> {
        let mut configured = Vec::new();
        for (index, package) in self.packages.iter().enumerate() {
            if package.mcp.is_empty() {
                continue;
            }
            let result = async {
                let directory = self.directory(index).await?;
                let host = self.host.clone();
                let name = package.summary.name.clone();
                let info = package.clone();
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
                                let options = config.options();
                                let launch = prepare(
                                    &host, &directory, &info, &server, config, &settings, helper,
                                );
                                (server, (options, launch))
                            })
                            .collect::<BTreeMap<_, _>>(),
                    )
                })
                .await
                .map_err(|_| unavailable())?
            }
            .await;
            let (mut servers, failure) = match result {
                Ok(servers) => (servers, None),
                Err(error) => (BTreeMap::new(), Some(error)),
            };
            for server in &package.mcp {
                let (options, mut launch) = servers.remove(&server.name).unwrap_or_else(|| {
                    (
                        Default::default(),
                        Err(failure.clone().unwrap_or_else(unavailable)),
                    )
                });
                if let Ok(Launch::Stdio {
                    command,
                    args,
                    env,
                    python,
                    ..
                }) = &mut launch
                    && matches!(command.to_str(), Some("python" | "python3"))
                {
                    match self.python_package(index, environment).await {
                        Ok(ready) => {
                            *command = ready.python.clone();
                            args.insert(0, "-B".into());
                            env.insert("PYTHONDONTWRITEBYTECODE".into(), "1".into());
                            let mut paths = vec![
                                ready
                                    .python
                                    .parent()
                                    .expect("managed Python has a parent")
                                    .to_owned(),
                            ];
                            if let Some(path) = env
                                .get("PATH")
                                .map(std::ffi::OsStr::new)
                                .or_else(|| environment.path())
                            {
                                paths.extend(std::env::split_paths(path));
                            }
                            match std::env::join_paths(paths)
                                .ok()
                                .and_then(|path| path.into_string().ok())
                            {
                                Some(path) => {
                                    env.insert("PATH".into(), path);
                                    *python = Some(ready);
                                }
                                None => {
                                    launch =
                                        Err(Fault::invalid("Python MCP PATH could not be encoded"));
                                }
                            }
                        }
                        Err(error) => {
                            launch = Err(error);
                        }
                    }
                }
                configured.push(Configured {
                    plugin: package.summary.name.clone(),
                    reference: package.summary.reference(),
                    name: server.name.clone(),
                    launch,
                    options,
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
        let closed = stop;
        let stop = closed.child_token();
        let _guard = stop.clone().drop_guard();
        let host = self.clone();
        let worker = tokio::task::spawn_blocking(move || host.mcp_configuration(&reference, stop));
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

    pub(crate) fn computer_launch(
        &self,
        info: &Info,
        settings: &crate::plugins::settings::Resolved,
        stop: CancellationToken,
    ) -> Result<Launch, Fault> {
        let directory = self.resolve(&info.summary.reference(), stop.clone())?;
        let mut declared = servers(&directory)?;
        let candidates: Vec<_> = declared.iter().filter_map(|(name, server)| {
            matches!(server, Server::Stdio { command, .. } if command == crate::computer::helper::TOKEN)
                .then_some(name.clone())
        }).collect();
        let [name] = candidates.as_slice() else {
            return Err(Fault::new(
                ErrorCode::NotConfigured,
                "computer helper declaration is unavailable",
            ));
        };
        if let Some(configuration) = &settings.mcp {
            mcp::validate_configuration(&declared, configuration)?;
            declared = configuration.servers.clone();
        }
        let server = declared.remove(name).ok_or_else(unavailable)?;
        if stop.is_cancelled() {
            return Err(cancelled());
        }
        if !matches!(&server, Server::Stdio { command, .. } if command == crate::computer::helper::TOKEN)
        {
            return Err(Fault::new(
                ErrorCode::NotConfigured,
                "computer helper declaration changed",
            ));
        }
        prepare(
            self,
            &directory,
            info,
            name,
            server,
            settings,
            crate::computer::helper::executable,
        )
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
    info: &Info,
    server_name: &str,
    server: Server,
    settings: &crate::plugins::settings::Resolved,
    helper: fn() -> Result<PathBuf, Fault>,
) -> Result<Launch, Fault> {
    let name = info.summary.name.as_str();
    let schema = info.settings.as_ref();
    let sse = matches!(&server, Server::Sse { .. });
    match server {
        Server::Http {
            url, mut headers, ..
        }
        | Server::Sse {
            url, mut headers, ..
        } => {
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
                    *value = format!(
                        "{}{}",
                        binding.prefix.as_deref().unwrap_or(""),
                        secret.expose()
                    );
                }
            }
            Ok(Launch::Http { url, headers, sse })
        }
        Server::Stdio {
            command,
            args,
            mut env,
            cwd,
            ..
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
            let command = if command == crate::computer::helper::TOKEN {
                helper()?
            } else if let Some(relative) = command.strip_prefix("./") {
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
                python: None,
            })
        }
    }
}

#[cfg(test)]
mod tests;
