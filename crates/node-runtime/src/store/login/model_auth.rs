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
    let sailry_protocol::conversation::oauth::Options::Plugin {
        ref package,
        ref flow,
        ref endpoint,
        ref consent,
        ref origin,
    } = options
    else {
        return Err(missing());
    };
    let info = crate::store::agent::plugins::packages(db, std::slice::from_ref(package))?
        .pop()
        .ok_or_else(missing)?;
    let extension = info.extension.as_ref().ok_or_else(missing)?;
    if !extension.actions.contains(&Action::AuthenticateModels) {
        return Err(missing());
    }
    let declaration = extension
        .model_auth
        .iter()
        .find(|declaration| declaration.id == *flow)
        .cloned()
        .filter(model_auth::Declaration::valid)
        .ok_or_else(missing)?;
    if declaration.exchange.is_some() && !extension.actions.contains(&Action::InspectCredentials) {
        return Err(missing());
    }
    if declaration.exchange.is_some() {
        crate::store::providers::consent::captured(
            consent.as_deref(),
            package,
            origin.as_ref(),
            sailry_protocol::conversation::consent::Scope::Credentials,
        )?;
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
        origin: origin.clone().ok_or_else(missing)?,
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
            origin: origin.clone().ok_or_else(missing)?,
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
        let Some(exchange) = &self.declaration.exchange else {
            return Ok(None);
        };
        let host = host.clone();
        let reference = self.capture.package.clone();
        let script = exchange.script.clone();
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
    progress: P,
    stop: CancellationToken,
) -> Result<Grant, Fault>
where
    P: FnOnce(login::State) -> F,
    F: std::future::Future<Output = Result<(), Fault>>,
{
    let configuration = prepared.configuration;
    let exchange = configuration.bundle(host, &stop).await?;
    let issued = flow::authorize(
        flow::Input {
            service,
            declaration: configuration.declaration,
            fields: configuration.target.fields.clone(),
            redirect: prepared.redirect,
            callback: prepared.callback,
            exchange,
        },
        progress,
        stop,
    )
    .await?;
    let grant = Grant::Plugin {
        record: Box::new(crate::providers::login::plugin::Record {
            target: configuration.target,
            issued,
        }),
        renewal_pending: false,
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

fn missing() -> Fault {
    Fault::new(
        ErrorCode::NotConfigured,
        "channel authentication plugin is unavailable or incomplete",
    )
}
