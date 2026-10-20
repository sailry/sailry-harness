//! Captures package authentication through the existing provider login worker.
use super::*;
use crate::providers::login::plugin::{Capture, Target, flow};
use rusqlite::Connection;
use sailry_protocol::plugin::{Action, model_auth};

pub(super) struct Configuration {
    pub capture: Capture,
    pub target: Target,
    pub declaration: model_auth::Declaration,
    pub options: sailry_protocol::conversation::oauth::Options,
}

pub(super) struct Prepared {
    pub configuration: Configuration,
    pub redirect: Option<String>,
    pub callback: Option<oneshot::Receiver<Secret>>,
    pub returned: Option<oneshot::Sender<Secret>>,
}

pub(super) fn configuration(
    db: &Connection,
    provider: &Provider,
    attempt: RequestId,
    options: Option<&sailry_protocol::conversation::oauth::Options>,
) -> Result<Configuration, Fault> {
    let mut captured = provider.clone();
    if options.is_none() {
        crate::store::providers::presets::capture_read(db, &mut captured)?;
    }
    let options = options
        .cloned()
        .map(Ok)
        .unwrap_or_else(|| crate::providers::login::effective(&captured))?;
    let (package, flow, endpoint, origin) = match &options {
        sailry_protocol::conversation::oauth::Options::Plugin {
            package,
            flow,
            endpoint,
            origin,
            ..
        } => (
            package.clone(),
            flow.clone(),
            endpoint.clone(),
            origin.clone().ok_or_else(missing)?,
        ),
        _ if managed(provider.authentication) => {
            let (info, declaration) = alias(db, provider.authentication)?;
            let origin = crate::store::providers::consent::origin(&info);
            (
                info.summary.reference(),
                declaration.id,
                provider.endpoint.clone(),
                origin,
            )
        }
        _ => return Err(missing()),
    };
    let info = crate::store::agent::plugins::packages(db, std::slice::from_ref(&package))?
        .pop()
        .ok_or_else(missing)?;
    let extension = info.extension.as_ref().ok_or_else(missing)?;
    if !extension.actions.contains(&Action::AuthenticateModels) {
        return Err(missing());
    }
    let declaration = extension
        .model_auth
        .iter()
        .find(|declaration| declaration.id == flow)
        .cloned()
        .filter(model_auth::Declaration::valid)
        .ok_or_else(missing)?;
    if (declaration.exchange.is_some()
        || matches!(declaration.flow, model_auth::Flow::Script { .. }))
        && !extension.actions.contains(&Action::InspectCredentials)
    {
        return Err(missing());
    }
    let resolved = crate::store::plugins::settings::resolve(db, &info)?;
    let mut fields = BTreeMap::new();
    let client_id = declaration.client.client_id.clone().or_else(|| {
        declaration
            .client
            .client_id_setting
            .as_ref()
            .and_then(|name| resolved.values.get(name))
            .and_then(serde_json::Value::as_str)
            .map(String::from)
    });
    if let Some(id) = client_id {
        fields.insert("client_id".into(), Secret::new(id));
    }
    if let Some(setting) = &declaration.client.client_secret_setting {
        fields.insert(
            "client_secret".into(),
            resolved.secrets.get(setting).cloned().ok_or_else(missing)?,
        );
    }
    let mut scopes = declaration.scopes.clone();
    scopes.sort();
    scopes.dedup();
    let target = Target {
        provider: provider.id,
        package: package.name.clone(),
        digest: origin.requires_digest().then(|| package.digest.clone()),
        origin: origin.clone(),
        flow: flow.clone(),
        endpoint: endpoint.clone(),
        authorization: declaration.flow.clone(),
        refresh: declaration
            .refresh
            .as_ref()
            .map(|refresh| refresh.request.clone()),
        inject: declaration.inject.clone(),
        fields,
        scopes,
    };
    Ok(Configuration {
        capture: Capture {
            attempt,
            provider: provider.id,
            revision: provider.revision,
            package: package.clone(),
            origin: origin.clone(),
            flow: flow.clone(),
        },
        target,
        declaration,
        options,
    })
}

pub(super) fn prepare(
    db: &Connection,
    provider: &Provider,
    attempt: RequestId,
    redirect: Option<&str>,
) -> Result<Prepared, Fault> {
    let configuration = configuration(db, provider, attempt, None)?;
    let (redirect, callback, returned) = match &configuration.declaration.flow {
        model_auth::Flow::Pkce {
            redirect: mode,
            redirect_uri,
            ..
        } => {
            let redirect = match mode {
                model_auth::Redirect::Loopback => redirect
                    .ok_or_else(|| {
                        Fault::invalid("model authorization requires a controller callback")
                    })?
                    .to_string(),
                model_auth::Redirect::Paste => {
                    if redirect.is_some() {
                        return Err(Fault::invalid(
                            "pasted authorization uses its declared callback",
                        ));
                    }
                    redirect_uri.clone().ok_or_else(missing)?
                }
            };
            let (returned, callback) = oneshot::channel();
            (Some(redirect), Some(callback), Some(returned))
        }
        model_auth::Flow::Script {
            callback: Some(mode),
            ..
        } => {
            let redirect = match redirect {
                Some(value) => value.to_string(),
                None if mode.manual => mode.redirect_uri.clone().ok_or_else(missing)?,
                None => {
                    return Err(Fault::invalid(
                        "model authorization requires a controller callback",
                    ));
                }
            };
            let (returned, callback) = oneshot::channel();
            (Some(redirect), Some(callback), Some(returned))
        }
        _ => {
            if redirect.is_some() {
                return Err(Fault::invalid(
                    "this model authorization does not use a callback",
                ));
            }
            (None, None, None)
        }
    };
    Ok(Prepared {
        configuration,
        redirect,
        callback,
        returned,
    })
}

impl Configuration {
    pub(super) async fn bundle(
        &self,
        host: &crate::plugins::Host,
        stop: &CancellationToken,
    ) -> Result<Option<crate::plugins::script::Bundle>, Fault> {
        let script = match &self.declaration.flow {
            model_auth::Flow::Script { script, .. } => script.clone(),
            _ => match &self.declaration.exchange {
                Some(exchange) => exchange.script.clone(),
                None => return Ok(None),
            },
        };
        let host = host.clone();
        let reference = self.capture.package.clone();
        let stop = stop.clone();
        tokio::task::spawn_blocking(move || {
            host.read_translation(&reference, &script, stop).map(Some)
        })
        .await
        .map_err(|_| missing())?
    }
}

pub(super) async fn authorize<P, F>(
    prepared: Prepared,
    service: crate::providers::login::Service,
    host: &crate::plugins::Host,
    context: Option<crate::providers::login::plugin::workflow::Context>,
    progress: P,
    stop: CancellationToken,
) -> Result<Grant, Fault>
where
    P: FnOnce(login::State) -> F,
    F: std::future::Future<Output = Result<(), Fault>>,
{
    let configuration = prepared.configuration;
    let exchange = configuration.bundle(host, &stop).await?;
    let mut fields = configuration.target.fields.clone();
    if configuration.declaration.authentication.is_some() {
        fields.insert(
            "user_agent".into(),
            Secret::new(configuration.options.user_agent().into()),
        );
    }
    let authentication = configuration.declaration.authentication;
    let issued = flow::authorize(
        flow::Input {
            service,
            declaration: configuration.declaration,
            fields,
            context,
            redirect: prepared.redirect,
            callback: prepared.callback,
            exchange,
        },
        progress,
        stop,
    )
    .await?;
    if let flow::Authorized::Grant(grant) = issued {
        grant.validate(authentication.ok_or_else(missing)?)?;
        return Ok(grant);
    }
    let flow::Authorized::Tokens(issued) = issued else {
        unreachable!()
    };
    let grant = Grant::Plugin {
        record: Box::new(crate::providers::login::plugin::Record {
            target: configuration.target,
            issued,
        }),
        renewal_pending: false,
        renewal_rejected: false,
    };
    grant.validate(Authentication::Plugin)?;
    Ok(grant)
}

pub(super) fn complete(
    db: &Connection,
    provider: &Provider,
    capture: &Capture,
) -> Result<(), Fault> {
    let current = configuration(db, provider, capture.attempt, None)?;
    capture.check(
        current.capture.attempt,
        current.capture.provider,
        current.capture.revision,
        &current.capture.package,
        &current.capture.origin,
        &current.capture.flow,
    )
}

pub(in crate::store) fn managed(authentication: Authentication) -> bool {
    matches!(
        authentication,
        Authentication::ChatGpt
            | Authentication::ChatGptOAuth
            | Authentication::ClaudeOAuth
            | Authentication::Plugin
    )
}

fn alias(
    db: &Connection,
    authentication: Authentication,
) -> Result<(sailry_protocol::plugin::Info, model_auth::Declaration), Fault> {
    let mut found = None;
    for summary in crate::store::plugins::list(db)? {
        if !summary.enabled {
            continue;
        }
        let Some(info) = crate::store::plugins::get(db, &summary.name)? else {
            continue;
        };
        if !info.issues.is_empty() {
            continue;
        }
        for declaration in info
            .extension
            .as_ref()
            .into_iter()
            .flat_map(|extension| &extension.model_auth)
        {
            if declaration.authentication != Some(authentication) || !declaration.valid() {
                continue;
            }
            if found.is_some() {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "model authentication alias is ambiguous",
                ));
            }
            found = Some((info.clone(), declaration.clone()));
        }
    }
    found.ok_or_else(missing)
}

pub(in crate::store) fn presentation(
    db: &Connection,
    authentication: Authentication,
) -> Result<Option<model_auth::Presentation>, Fault> {
    if !managed(authentication) || authentication == Authentication::Plugin {
        return Ok(None);
    }
    match alias(db, authentication) {
        Ok((_, declaration)) => Ok(Some(declaration.flow.presentation())),
        Err(error) if error.code == ErrorCode::NotConfigured => Ok(None),
        Err(error) => Err(error),
    }
}

pub(in crate::store) fn legacy_target(
    db: &Connection,
    provider: &Provider,
    package: &str,
    flow: &str,
) -> Result<Target, Fault> {
    let current = configuration(db, provider, RequestId::new(), None)?;
    if current.target.package != package
        || current.target.flow != flow
        || current.target.origin != sailry_protocol::conversation::consent::Origin::Bundled
    {
        return Err(missing());
    }
    Ok(current.target)
}

pub(in crate::store) fn check_binding(
    db: &Connection,
    credential: &Credential,
    target: &Target,
) -> Result<(), Fault> {
    let saved =
        crate::store::providers::authentication::binding(db, credential.id, credential.provider)?
            .ok_or_else(missing)?;
    if saved.origin != target.origin {
        return Err(missing());
    }
    saved.matches(target)
}

fn missing() -> Fault {
    Fault::new(
        ErrorCode::NotConfigured,
        "channel authentication plugin is unavailable or incomplete",
    )
}
