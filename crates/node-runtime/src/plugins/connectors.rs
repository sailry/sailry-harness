//! Declaration-driven connection execution; secrets never enter package JavaScript.
mod cli;
pub(crate) mod mcp;
use crate::providers::login::{
    self,
    plugin::{Issued, flow},
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault, Secret,
    plugin::{Reference, connector::*, model_auth},
};
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

#[derive(Clone)]
pub(crate) struct Target {
    pub id: Id,
    /// Authorization fence, independent from public editing revisions.
    pub revision: u64,
    pub package: Reference,
    pub declaration: Declaration,
    pub method: Kind,
    pub fields: BTreeMap<String, String>,
    pub secrets: BTreeMap<String, Secret>,
    pub identity: Option<Identity>,
    pub grant: Option<Issued>,
    pub mcp_endpoint: Option<String>,
    pub manual: bool,
    pub checking: bool,
}

pub(super) fn validate(
    extension: &sailry_protocol::plugin::Extension,
    servers: &BTreeMap<String, super::mcp::Server>,
    schema: Option<&sailry_protocol::plugin::settings::Schema>,
) -> bool {
    extension.connectors.iter().all(|declaration| {
        declaration.fields.iter().all(|field| {
            schema
                .and_then(|schema| schema.properties.get(&field.name))
                .is_none_or(|field| field.secret.is_none())
        }) && declaration.inject.iter().all(|injection| {
            match injection
                .server
                .as_ref()
                .and_then(|server| servers.get(server))
            {
                None => injection.server.is_none(),
                Some(
                    super::mcp::Server::Http { url, .. } | super::mcp::Server::Sse { url, .. },
                ) => {
                    injection.header.is_some()
                        && injection.env.is_none()
                        && declaration.endpoint(url)
                }
                Some(super::mcp::Server::Stdio { .. }) => {
                    injection.env.is_some() && injection.header.is_none()
                }
            }
        }) && declaration.auth.iter().all(|auth| match auth {
            Auth::McpOauth { server, .. } => {
                matches!(
                    servers.get(server),
                    Some(super::mcp::Server::Http { .. } | super::mcp::Server::Sse { .. })
                ) && declaration.inject.iter().any(|injection| {
                    injection.server.as_ref() == Some(server)
                        && injection
                            .header
                            .as_deref()
                            .is_some_and(|header| header.eq_ignore_ascii_case("authorization"))
                })
            }
            _ => true,
        })
    })
}

impl Target {
    pub(crate) fn endpoint(&self, url: &str) -> bool {
        self.declaration.endpoint(url)
            || self.manual
                && super::mcp::validate_http(url, &Default::default()).is_ok()
                && url::Url::parse(url).is_ok_and(|url| {
                    self.declaration
                        .origins
                        .contains(&url.origin().ascii_serialization())
                })
    }
}

#[derive(Default)]
pub(crate) struct Service {
    locks: Mutex<BTreeMap<Id, Weak<tokio::sync::Mutex<()>>>>,
    cli: Mutex<BTreeMap<Id, Cached>>,
    #[cfg(any(test, feature = "test-support"))]
    fixtures: Mutex<BTreeMap<String, String>>,
}

struct Cached {
    revision: u64,
    auth: Auth,
    identity: Identity,
    expires: tokio::time::Instant,
    token: Secret,
}

pub(crate) struct Login {
    pub target: Target,
    pub input: Input,
    pub host: super::Host,
    pub service: login::Service,
    pub environment: Arc<crate::process::Environment>,
    pub callback: Option<tokio::sync::oneshot::Receiver<Secret>>,
    pub stop: CancellationToken,
}

impl Service {
    pub(crate) async fn inspect_cli(
        &self,
        declaration: &Declaration,
        environment: &crate::process::Environment,
        stop: &CancellationToken,
    ) -> Result<Identity, Fault> {
        let auth = declaration
            .auth
            .iter()
            .find(|auth| auth.kind() == Kind::LocalCli)
            .ok_or_else(unavailable)?;
        cli::identity(auth, environment, stop).await
    }
    pub(crate) fn lock(&self, id: Id) -> Result<Arc<tokio::sync::Mutex<()>>, Fault> {
        let mut locks = self.locks.lock().map_err(|_| unavailable())?;
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(&id).and_then(Weak::upgrade) {
            return Ok(lock);
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(id, Arc::downgrade(&lock));
        Ok(lock)
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn fixture(&self, origin: &str, url: &str) {
        let url = url::Url::parse(url).expect("connector fixture URL is valid");
        assert!(
            url.scheme() == "http" && url.host_str() == Some("127.0.0.1") && url.port().is_some()
        );
        self.fixtures
            .lock()
            .unwrap()
            .insert(origin.into(), url.origin().ascii_serialization());
    }
    pub(crate) fn resource(&self, url: &str) -> String {
        #[cfg(any(test, feature = "test-support"))]
        if let Ok(parsed) = url::Url::parse(url)
            && let Some(origin) = self
                .fixtures
                .lock()
                .unwrap()
                .get(&parsed.origin().ascii_serialization())
        {
            return format!(
                "{origin}{}{}",
                parsed.path(),
                parsed
                    .query()
                    .map(|query| format!("?{query}"))
                    .unwrap_or_default()
            );
        }
        url.into()
    }
    pub(crate) async fn access(
        &self,
        target: &Target,
        ingress: &crate::store::Ingress,
        rejected: Option<&Secret>,
        stop: &CancellationToken,
    ) -> Result<Secret, Fault> {
        let lock = self.lock(target.id)?;
        let _guard = tokio::select! {biased;_=stop.cancelled()=>return Err(cancelled()),guard=lock.lock()=>guard};
        let live = ingress
            .connector_target(target.id, target.revision, target.checking)
            .await?;
        let mut target = target.clone();
        target.grant = live.grant;
        let auth = target
            .declaration
            .auth
            .iter()
            .find(|auth| auth.kind() == target.method)
            .ok_or_else(unavailable)?;
        match auth {
            Auth::Secret { fields } => fields
                .first()
                .and_then(|field| target.secrets.get(field))
                .cloned()
                .ok_or_else(unavailable),
            Auth::LocalCli { timeout_ms, .. } => {
                let expected = target.identity.as_ref().ok_or_else(unavailable)?;
                if !target.checking
                    && let Some(cached) =
                        self.cli.lock().map_err(|_| unavailable())?.get(&target.id)
                    && cached.revision == target.revision
                    && cached.auth == *auth
                    && cached.identity == *expected
                    && cached.expires > tokio::time::Instant::now()
                    && rejected != Some(&cached.token)
                {
                    return Ok(cached.token.clone());
                }
                self.cli
                    .lock()
                    .map_err(|_| unavailable())?
                    .remove(&target.id);
                let result =
                    cli::credentials(auth, &ingress.environment, Some(&expected.id), stop).await;
                let (identity, token) = match result {
                    Ok(value) if value.0.id == expected.id => value,
                    Ok(_) => {
                        ingress
                            .save_connector_grant(target.id, target.revision, None, None)
                            .await?;
                        return Err(unavailable());
                    }
                    Err(error) if error.code == ErrorCode::Expired => {
                        ingress
                            .save_connector_grant(target.id, target.revision, None, None)
                            .await?;
                        return Err(error);
                    }
                    Err(error) => return Err(error),
                };
                self.cli.lock().map_err(|_| unavailable())?.insert(
                    target.id,
                    Cached {
                        revision: target.revision,
                        auth: auth.clone(),
                        identity,
                        expires: tokio::time::Instant::now() + Duration::from_millis(*timeout_ms),
                        token: token.clone(),
                    },
                );
                Ok(token)
            }
            Auth::Application { .. } | Auth::Oauth2 { .. } => {
                if let Some(grant) = &target.grant
                    && rejected != Some(&grant.access)
                    && grant.expires_at_ms.is_none_or(|expires| expires > now())
                {
                    return Ok(grant.access.clone());
                }
                let (declaration, fields) = self.declaration(&target, false)?;
                let service = self.transport(ingress.login_service.clone(), &declaration);
                let issued = if let Some(previous) = &target.grant
                    && matches!(auth, Auth::Oauth2 { .. })
                {
                    flow::refresh(
                        &service,
                        &declaration,
                        &fields,
                        previous,
                        None,
                        stop.clone(),
                    )
                    .await
                } else {
                    flow::authorize(
                        flow::Input {
                            context: None,
                            service,
                            declaration,
                            fields,
                            redirect: None,
                            callback: None,
                            exchange: None,
                        },
                        |_| async { Err(unavailable()) },
                        stop.clone(),
                    )
                    .await
                    .and_then(tokens)
                };
                match issued {
                    Ok(issued) => {
                        ingress
                            .save_connector_grant(
                                target.id,
                                target.revision,
                                target.grant.as_ref().map(|grant| grant.access.clone()),
                                Some(issued.clone()),
                            )
                            .await?;
                        Ok(issued.access)
                    }
                    Err(error) if error.code == ErrorCode::PermissionDenied => {
                        ingress
                            .save_connector_grant(
                                target.id,
                                target.revision,
                                target.grant.as_ref().map(|grant| grant.access.clone()),
                                None,
                            )
                            .await?;
                        Err(error)
                    }
                    Err(error) => Err(error),
                }
            }
            Auth::McpOauth { .. } => target
                .grant
                .as_ref()
                .map(|grant| grant.access.clone())
                .ok_or_else(unavailable),
        }
    }
    pub(crate) fn declaration(
        &self,
        target: &Target,
        registration: bool,
    ) -> Result<(model_auth::Declaration, BTreeMap<String, Secret>), Fault> {
        let auth = target
            .declaration
            .auth
            .iter()
            .find(|auth| auth.kind() == target.method)
            .ok_or_else(unavailable)?;
        let mut fields = target
            .fields
            .iter()
            .map(|(name, value)| (name.clone(), Secret::new(value.clone())))
            .collect::<BTreeMap<_, _>>();
        fields.extend(target.secrets.clone());
        let (flow, token, refresh, scopes) = match auth {
            Auth::Oauth2 {
                flow,
                authorize,
                token,
                device,
                client,
                scopes,
                refresh,
            } => {
                client_fields(client, target, &mut fields)?;
                let token_url = expanded(&target.declaration, token, &target.fields)?;
                let mut token_endpoint = endpoint(token_url);
                let mut body = BTreeMap::from([("client_id".into(), "{client_id}".into())]);
                if fields.contains_key("client_secret") {
                    body.insert("client_secret".into(), "{client_secret}".into());
                }
                token_endpoint.body = Some(model_auth::Body::Form { fields: body });
                let mapping = model_auth::Mapping {
                    access: "/access_token".into(),
                    refresh: Some("/refresh_token".into()),
                    expires_in: Some("/expires_in".into()),
                    account_label: None,
                    extra: BTreeMap::new(),
                };
                let refresh = refresh.then(|| model_auth::Refresh {
                    request: token_endpoint.clone(),
                    token: mapping.clone(),
                });
                let flow = match flow {
                    Flow::Pkce => model_auth::Flow::Pkce {
                        authorize: expanded(&target.declaration, authorize, &target.fields)?,
                        token: token_endpoint,
                        redirect: model_auth::Redirect::Loopback,
                        redirect_uri: None,
                    },
                    Flow::DeviceCode => model_auth::Flow::DeviceCode {
                        device: endpoint(expanded(
                            &target.declaration,
                            device.as_ref().ok_or_else(unavailable)?,
                            &target.fields,
                        )?),
                        token: token_endpoint,
                    },
                };
                (flow, mapping, refresh, scopes.clone())
            }
            Auth::Application {
                fields: client,
                token: _,
                registration: Some(registered),
            } if registration => {
                client_values(client, target, &mut fields, false)?;
                (
                    registered.flow.clone(),
                    registered.token.clone(),
                    None,
                    registered.scopes.clone(),
                )
            }
            Auth::Application {
                fields: client,
                token,
                ..
            } => {
                client_values(client, target, &mut fields, true)?;
                let mut endpoint =
                    endpoint(expanded(&target.declaration, &token.url, &target.fields)?);
                endpoint.body = Some(if token.form {
                    model_auth::Body::Form {
                        fields: token.body.clone(),
                    }
                } else {
                    model_auth::Body::Json {
                        value: serde_json::to_value(&token.body).map_err(|_| unavailable())?,
                    }
                });
                (
                    model_auth::Flow::TokenExchange { token: endpoint },
                    model_auth::Mapping {
                        access: token.access.clone(),
                        refresh: None,
                        expires_in: Some(token.expires_in.clone()),
                        account_label: None,
                        extra: BTreeMap::new(),
                    },
                    None,
                    Vec::new(),
                )
            }
            _ => return Err(unavailable()),
        };
        Ok((
            model_auth::Declaration {
                id: target.declaration.id.clone(),
                authentication: None,
                display: target.declaration.display.clone(),
                flow,
                client: Default::default(),
                scopes,
                token: Some(token),
                inject: vec![model_auth::Injection {
                    header: "Authorization".into(),
                    template: "Bearer {access}".into(),
                }],
                refresh,
                exchange: None,
                requests: BTreeMap::new(),
                mutations: BTreeMap::new(),
            },
            fields,
        ))
    }
    fn transport(
        &self,
        service: login::Service,
        _declaration: &model_auth::Declaration,
    ) -> login::Service {
        #[cfg(any(test, feature = "test-support"))]
        {
            let mut service = service;
            let endpoint = match &_declaration.flow {
                model_auth::Flow::Script { requests, .. } => requests
                    .values()
                    .next()
                    .map(|endpoint| endpoint.url.as_str()),
                model_auth::Flow::Pkce { token, .. }
                | model_auth::Flow::Polling { token, .. }
                | model_auth::Flow::DeviceCode { token, .. }
                | model_auth::Flow::TokenExchange { token } => Some(token.url.as_str()),
            };
            if let Some(endpoint) = endpoint {
                let routed = self.resource(endpoint);
                if routed != endpoint {
                    service.model_endpoint = Some(routed);
                }
            }
            service
        }
        #[cfg(not(any(test, feature = "test-support")))]
        service
    }

    pub(crate) async fn authorize<P, F>(&self, login: Login, progress: P) -> Result<Target, Fault>
    where
        P: FnOnce(sailry_protocol::conversation::login::State) -> F,
        F: Future<Output = Result<(), Fault>>,
    {
        let Login {
            mut target,
            input,
            host,
            service,
            environment,
            callback,
            stop,
        } = login;
        let auth = target
            .declaration
            .auth
            .iter()
            .find(|auth| auth.kind() == target.method)
            .cloned()
            .ok_or_else(unavailable)?;
        match &auth {
            Auth::Secret { fields } => {
                if !fields.iter().all(|field| {
                    target
                        .declaration
                        .fields
                        .iter()
                        .any(|declared| declared.name == *field && declared.optional)
                        || target
                            .secrets
                            .get(field)
                            .is_some_and(|secret| !secret.expose().is_empty())
                }) {
                    return Err(unavailable());
                }
            }
            Auth::LocalCli { .. } => {
                let (identity, access) = cli::credentials(&auth, &environment, None, &stop).await?;
                target.identity = Some(identity);
                target.grant = Some(Issued {
                    account_label: None,
                    access,
                    refresh: None,
                    expires_at_ms: None,
                    extra: BTreeMap::new(),
                });
            }
            Auth::McpOauth { server: _, client } => {
                let endpoint = target.mcp_endpoint.as_deref().ok_or_else(unavailable)?;
                let endpoint = self.resource(endpoint);
                let mut fields = BTreeMap::new();
                if let Some(client) = client {
                    client_fields(client, &target, &mut fields)?;
                }
                let id = fields.get("client_id").map(|value| value.expose());
                let registered =
                    fields
                        .get("client_secret")
                        .map(|secret| super::authorization::Registered {
                            id: id.unwrap_or_default().into(),
                            secret: secret.clone(),
                        });
                let redirect = input.redirect.as_deref().ok_or_else(unavailable)?;
                let pending = super::authorization::Flow::begin(
                    &endpoint,
                    redirect,
                    id,
                    registered,
                    target.grant.as_ref().map(|grant| &grant.access),
                )
                .await?;
                progress(sailry_protocol::conversation::login::State::Browser {
                    authorization_url: pending.url().into(),
                    expires_at_ms: now() + 900_000,
                })
                .await?;
                let callback = tokio::select! {biased;_=stop.cancelled()=>return Err(cancelled()),callback=callback.ok_or_else(unavailable)?=>callback.map_err(|_|cancelled())?};
                let access = pending.complete(&callback).await?;
                target.grant = Some(Issued {
                    account_label: None,
                    access,
                    refresh: None,
                    expires_at_ms: None,
                    extra: BTreeMap::new(),
                });
            }
            Auth::Oauth2 { .. } | Auth::Application { .. } => {
                let (declaration, fields) = self.declaration(&target, input.registration)?;
                let exchange = if let model_auth::Flow::Script { script, .. } = &declaration.flow {
                    Some(host.read_translation(&target.package, script, stop.clone())?)
                } else {
                    None
                };
                let interactive = matches!(
                    declaration.flow,
                    model_auth::Flow::Pkce { .. } | model_auth::Flow::Script { .. }
                );
                let issued = flow::authorize(
                    flow::Input {
                        context: None,
                        service: self.transport(service.clone(), &declaration),
                        declaration,
                        fields,
                        redirect: interactive.then(|| input.redirect.clone()).flatten(),
                        callback: if interactive { callback } else { None },
                        exchange,
                    },
                    progress,
                    stop.clone(),
                )
                .await
                .and_then(tokens)?;
                if input.registration {
                    let Auth::Application {
                        registration: Some(registration),
                        ..
                    } = &auth
                    else {
                        return Err(unavailable());
                    };
                    let id = issued
                        .extra
                        .get("client_id")
                        .ok_or_else(unavailable)?
                        .expose()
                        .to_owned();
                    target
                        .fields
                        .insert(registration.issues.client_id.clone(), id);
                    target.secrets.insert(
                        registration
                            .issues
                            .client_secret
                            .clone()
                            .ok_or_else(unavailable)?,
                        issued.access,
                    );
                    let (declaration, fields) = self.declaration(&target, false)?;
                    target.grant = Some(
                        flow::authorize(
                            flow::Input {
                                context: None,
                                service: self.transport(service, &declaration),
                                declaration,
                                fields,
                                redirect: None,
                                callback: None,
                                exchange: None,
                            },
                            |_| async { Err(unavailable()) },
                            stop.clone(),
                        )
                        .await
                        .and_then(tokens)?,
                    );
                } else {
                    target.grant = Some(issued);
                }
            }
        }
        if target.declaration.identity.is_some() || target.declaration.health.is_some() {
            self.probe(&mut target, &stop).await?;
        }
        if target.method == Kind::LocalCli {
            target.grant = None;
        }
        Ok(target)
    }
    pub(crate) async fn probe(
        &self,
        target: &mut Target,
        stop: &CancellationToken,
    ) -> Result<(), Fault> {
        let failed = || Fault::new(ErrorCode::Unavailable, "connector check failed");
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| failed())?;
        for (url, identity, ok) in target
            .declaration
            .identity
            .iter()
            .map(|item| (&item.request, Some(item), vec![200]))
            .chain(
                target
                    .declaration
                    .health
                    .iter()
                    .map(|item| (&item.request, None, item.ok.clone())),
            )
        {
            let url = expanded(&target.declaration, url, &target.fields)?;
            let injection = target.declaration.inject.iter().find(|item| {
                item.origin.as_ref().is_some_and(|origin| {
                    url::Url::parse(&url)
                        .is_ok_and(|url| url.origin().ascii_serialization() == *origin)
                })
            });
            let mut request = client.get(self.resource(&url));
            let mut protected = Vec::new();
            if let Some(injection) = injection
                && let Some(header) = &injection.header
            {
                let oauth = if target.method == Kind::McpOauth {
                    target
                        .grant
                        .as_ref()
                        .map(|grant| super::authorization::access(&grant.access))
                        .transpose()?
                } else {
                    None
                };
                let token = oauth
                    .as_ref()
                    .or_else(|| {
                        target
                            .grant
                            .as_ref()
                            .map(|grant| &grant.access)
                            .or_else(|| {
                                injection
                                    .field
                                    .as_ref()
                                    .and_then(|field| target.secrets.get(field))
                            })
                            .or_else(|| target.secrets.values().next())
                    })
                    .ok_or_else(unavailable)?;
                protected.push(token.clone());
                request = request.header(header, format!("{}{}", injection.prefix, token.expose()));
            }
            let mut response = tokio::select! {biased;_=stop.cancelled()=>return Err(cancelled()),response=request.send()=>response.map_err(|_|failed())?};
            if response.status().as_u16() == 401 {
                return Err(Fault::new(
                    ErrorCode::Expired,
                    "connector authorization expired",
                ));
            }
            if !ok.contains(&response.status().as_u16()) {
                return Err(failed());
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| failed())? {
                if bytes.len().saturating_add(chunk.len()) > 65_536 {
                    return Err(failed());
                }
                bytes.extend_from_slice(&chunk);
            }
            if let Some(identity) = identity {
                let value: serde_json::Value =
                    serde_json::from_slice(&bytes).map_err(|_| failed())?;
                let identity = Identity {
                    name: value
                        .pointer(&identity.name)
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(unavailable)?
                        .into(),
                    id: value
                        .pointer(&identity.id)
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(unavailable)?
                        .into(),
                };
                if [&identity.name, &identity.id].into_iter().any(|value| {
                    value.is_empty()
                        || value.len() > 256
                        || value.chars().any(char::is_control)
                        || protected.iter().any(|secret| {
                            !secret.expose().is_empty() && value.contains(secret.expose())
                        })
                        || target
                            .secrets
                            .values()
                            .chain(target.grant.iter().flat_map(|grant| {
                                std::iter::once(&grant.access)
                                    .chain(grant.refresh.iter())
                                    .chain(grant.extra.values())
                            }))
                            .any(|secret| {
                                !secret.expose().is_empty() && value.contains(secret.expose())
                            })
                }) {
                    return Err(unavailable());
                }
                target.identity = Some(identity);
            }
        }
        Ok(())
    }
}

fn endpoint(url: String) -> model_auth::Endpoint {
    model_auth::Endpoint {
        cache_ttl_ms: None,
        url,
        method: model_auth::Method::Post,
        headers: BTreeMap::new(),
        query: BTreeMap::new(),
        body: None,
    }
}
fn client_fields(
    client: &Client,
    target: &Target,
    fields: &mut BTreeMap<String, Secret>,
) -> Result<(), Fault> {
    match client {
        Client::Bundled { bundled } => {
            fields.insert("client_id".into(), Secret::new(bundled.client_id.clone()));
            Ok(())
        }
        Client::Fields { fields: client } => client_values(client, target, fields, true),
    }
}
fn client_values(
    client: &ClientFields,
    target: &Target,
    fields: &mut BTreeMap<String, Secret>,
    required: bool,
) -> Result<(), Fault> {
    if let Some(id) = target.fields.get(&client.client_id) {
        fields.insert("client_id".into(), Secret::new(id.clone()));
    } else if required {
        return Err(unavailable());
    }
    if let Some(name) = &client.client_secret {
        if let Some(value) = target.secrets.get(name) {
            fields.insert("client_secret".into(), value.clone());
        } else if required {
            return Err(unavailable());
        }
    }
    Ok(())
}
pub(crate) fn expanded(
    declaration: &Declaration,
    url: &str,
    fields: &BTreeMap<String, String>,
) -> Result<String, Fault> {
    let mut result = url.to_owned();
    for field in declaration.fields.iter().filter(|field| !field.secret) {
        let placeholder = format!("{{{}}}", field.name);
        if result.contains(&placeholder) {
            let value = fields
                .get(&field.name)
                .or(field.default.as_ref())
                .ok_or_else(unavailable)?;
            if value.is_empty()
                || value.len() > 128
                || !value
                    .bytes()
                    .all(|value| value.is_ascii_alphanumeric() || b".-".contains(&value))
            {
                return Err(Fault::invalid("invalid connector endpoint field"));
            }
            result = result.replace(&placeholder, value);
        }
    }
    if !declaration.endpoint(&result) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "connector endpoint is outside its declared origins",
        ));
    }
    Ok(result)
}
fn tokens(value: flow::Authorized) -> Result<Issued, Fault> {
    match value {
        flow::Authorized::Tokens(tokens) => Ok(tokens),
        _ => Err(unavailable()),
    }
}
pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn unavailable() -> Fault {
    Fault::new(
        ErrorCode::NotConfigured,
        "connector is unavailable or changed",
    )
}
fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "connector operation cancelled")
}
