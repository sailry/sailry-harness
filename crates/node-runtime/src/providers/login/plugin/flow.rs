//! Standard flows reuse the Node's login deadlines, callback and HTTP owners.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sailry_protocol::conversation::login::State;
use sha2::{Digest, Sha256};
use std::{future::Future, time::Duration};
use tokio::sync::oneshot;

#[cfg(test)]
mod tests;

pub(crate) struct Input {
    pub service: super::super::Service,
    pub declaration: model_auth::Declaration,
    pub fields: BTreeMap<String, Secret>,
    pub redirect: Option<String>,
    pub callback: Option<oneshot::Receiver<Secret>>,
    pub exchange: Option<crate::plugins::script::Bundle>,
}

pub(crate) async fn authorize<P, F>(
    mut input: Input,
    progress: P,
    stop: CancellationToken,
) -> Result<Issued, Fault>
where
    P: FnOnce(State) -> F,
    F: Future<Output = Result<(), Fault>>,
{
    if !input.declaration.valid() {
        return Err(invalid());
    }
    let operation = async {
        let document = match input.declaration.flow.clone() {
            model_auth::Flow::Polling {
                start,
                state,
                verification_url,
                token,
                success,
                interval_ms,
            } => {
                if input.callback.is_some() || input.redirect.is_some() {
                    return Err(invalid());
                }
                let started = request(&input.service, &start, &input.fields, &stop).await?;
                let state = text(&started, &state)?.ok_or_else(invalid)?;
                let url = text(&started, &verification_url)?.ok_or_else(invalid)?;
                if !(model_auth::Endpoint {
                    url: url.expose().into(),
                    method: model_auth::Method::Get,
                    headers: BTreeMap::new(),
                    query: BTreeMap::new(),
                    body: None,
                })
                .valid()
                {
                    return Err(invalid());
                }
                progress(State::Browser {
                    authorization_url: url.expose().into(),
                    expires_at_ms: super::super::now_ms()?
                        + super::super::DEADLINE.as_millis() as u64,
                })
                .await?;
                input.fields.insert("state".into(), state);
                let interval = interval_ms
                    .map(Duration::from_millis)
                    .unwrap_or(Duration::from_secs(5));
                loop {
                    tokio::select! {biased; _ = stop.cancelled() => return Err(cancelled()), _ = tokio::time::sleep(interval) => {}}
                    let (status, value) =
                        response(&input.service, &token, &input.fields, &stop).await?;
                    if status.is_success()
                        && value
                            .pointer(&success.pointer)
                            .is_some_and(|value| success.values.contains(value))
                    {
                        break value;
                    }
                    if !status.is_success() {
                        return Err(super::super::http_error(status));
                    }
                }
            }
            model_auth::Flow::Pkce {
                authorize,
                token,
                redirect,
                redirect_uri: _,
            } => {
                let callback = input.callback.take().ok_or_else(invalid)?;
                let redirect_uri = input.redirect.take().ok_or_else(invalid)?;
                validate_redirect(&redirect_uri, redirect)?;
                let pending = Pending::new(
                    authorize,
                    redirect_uri,
                    &input.fields,
                    &input.declaration.scopes,
                )?;
                progress(State::Browser {
                    authorization_url: pending.url.clone(),
                    expires_at_ms: super::super::now_ms()?
                        + super::super::DEADLINE.as_millis() as u64,
                })
                .await?;
                let callback = tokio::select! {
                    biased;
                    _ = stop.cancelled() => return Err(cancelled()),
                    result = callback => result.map_err(|_| cancelled())?,
                };
                let code = pending.code(&callback)?;
                input.fields.insert("code".into(), code);
                input
                    .fields
                    .insert("redirect_uri".into(), Secret::new(pending.redirect));
                input.fields.insert("verifier".into(), pending.verifier);
                let endpoint = form(
                    token,
                    &[
                        ("grant_type", "authorization_code"),
                        ("code", "{code}"),
                        ("redirect_uri", "{redirect_uri}"),
                        ("code_verifier", "{verifier}"),
                    ],
                    &input.fields,
                );
                request(&input.service, &endpoint, &input.fields, &stop).await?
            }
            model_auth::Flow::DeviceCode { device, token } => {
                if input.callback.is_some() || input.redirect.is_some() {
                    return Err(invalid());
                }
                if !input.declaration.scopes.is_empty() {
                    input.fields.insert(
                        "scope".into(),
                        Secret::new(input.declaration.scopes.join(" ")),
                    );
                }
                let device = form(device, &[], &input.fields);
                let document = request(&input.service, &device, &input.fields, &stop).await?;
                let pending = Device::parse(&document, super::super::now_ms()?)?;
                progress(State::Pending {
                    verification_url: pending.url,
                    user_code: pending.user_code,
                    expires_at_ms: pending.expires_at_ms,
                })
                .await?;
                input.fields.insert("device_code".into(), pending.code);
                let token = form(
                    token,
                    &[
                        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                        ("device_code", "{device_code}"),
                    ],
                    &input.fields,
                );
                poll(
                    &input.service,
                    &token,
                    &input.fields,
                    pending.interval,
                    pending.expires_at_ms,
                    &stop,
                )
                .await?
            }
            model_auth::Flow::TokenExchange { token } => {
                if input.callback.is_some() || input.redirect.is_some() {
                    return Err(invalid());
                }
                request(&input.service, &token, &input.fields, &stop).await?
            }
        };
        decode(
            document,
            &input.declaration,
            input.exchange.take(),
            None,
            stop.clone(),
        )
        .await
    };
    tokio::select! {
        biased;
        _ = stop.cancelled() => Err(cancelled()),
        result = tokio::time::timeout(super::super::DEADLINE, operation) => result.map_err(|_| super::super::expired())?,
    }
}

pub(crate) async fn refresh(
    service: &super::super::Service,
    declaration: &model_auth::Declaration,
    fields: &BTreeMap<String, Secret>,
    previous: &Issued,
    exchange: Option<crate::plugins::script::Bundle>,
    stop: CancellationToken,
) -> Result<Issued, Fault> {
    let refresh = declaration.refresh.as_ref().ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "model sign-in expired; sign in again",
        )
    })?;
    let mut fields = fields.clone();
    fields.extend(previous.fields());
    if previous.refresh.is_none() {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "model sign-in cannot be refreshed; sign in again",
        ));
    }
    let endpoint = match &declaration.flow {
        model_auth::Flow::Pkce { .. } | model_auth::Flow::DeviceCode { .. } => form(
            refresh.request.clone(),
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", "{refresh}"),
            ],
            &fields,
        ),
        // A fixed polling or exchange contract does not imply OAuth form fields.
        _ => refresh.request.clone(),
    };
    let document = refresh_request(service, &endpoint, &fields, &stop).await?;
    let mut declaration = declaration.clone();
    declaration.token = refresh.token.clone();
    decode(document, &declaration, exchange, Some(previous), stop).await
}

async fn decode(
    document: Value,
    declaration: &model_auth::Declaration,
    bundle: Option<crate::plugins::script::Bundle>,
    previous: Option<&Issued>,
    stop: CancellationToken,
) -> Result<Issued, Fault> {
    let document = if let Some(exchange) = &declaration.exchange {
        let bundle = bundle.ok_or_else(invalid)?;
        let handler = exchange.handler.clone();
        tokio::task::spawn_blocking(move || {
            let mut session = crate::plugins::script::translation::Session::new(bundle, stop)?;
            session.call(&handler, &document, super::super::BODY_LIMIT)
        })
        .await
        .map_err(|_| invalid())??
    } else {
        if bundle.is_some() {
            return Err(invalid());
        }
        document
    };
    Issued::parse(
        &document,
        &declaration.token,
        super::super::now_ms()?,
        previous,
    )
}

fn form(
    mut endpoint: model_auth::Endpoint,
    values: &[(&str, &str)],
    fields: &BTreeMap<String, Secret>,
) -> model_auth::Endpoint {
    if endpoint.body.is_none() {
        let mut body: BTreeMap<String, String> = values
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        for name in ["client_id", "client_secret", "scope"] {
            if fields.contains_key(name) {
                body.insert(name.into(), format!("{{{name}}}"));
            }
        }
        endpoint.body = Some(model_auth::Body::Form { fields: body });
    }
    endpoint
}

struct Pending {
    url: String,
    redirect: String,
    verifier: Secret,
    state: Secret,
}

impl Pending {
    fn new(
        authorize: String,
        redirect: String,
        fields: &BTreeMap<String, Secret>,
        scopes: &[String],
    ) -> Result<Self, Fault> {
        let mut url = Url::parse(&authorize).map_err(|_| invalid())?;
        let client = fields.get("client_id").ok_or_else(invalid)?;
        let verifier = super::super::chatgpt::flow::random();
        let state = super::super::chatgpt::flow::random();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.expose().as_bytes()));
        let reserved = [
            "client_id",
            "redirect_uri",
            "response_type",
            "state",
            "code_challenge",
            "code_challenge_method",
            "scope",
        ];
        if url
            .query_pairs()
            .any(|(key, _)| reserved.contains(&key.as_ref()))
        {
            return Err(invalid());
        }
        url.query_pairs_mut().extend_pairs([
            ("client_id", client.expose()),
            ("redirect_uri", redirect.as_str()),
            ("response_type", "code"),
            ("state", state.expose()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ]);
        if !scopes.is_empty() {
            url.query_pairs_mut()
                .append_pair("scope", &scopes.join(" "));
        }
        Ok(Self {
            url: url.into(),
            redirect,
            verifier,
            state,
        })
    }

    fn code(&self, callback: &Secret) -> Result<Secret, Fault> {
        let mut url = Url::parse(callback.expose()).map_err(|_| invalid())?;
        let mut values = BTreeMap::new();
        for (key, value) in url.query_pairs() {
            if values
                .insert(key.into_owned(), value.into_owned())
                .is_some()
            {
                return Err(invalid());
            }
        }
        if url.fragment().is_some() {
            return Err(invalid());
        }
        url.set_query(None);
        if url.as_str() != self.redirect
            || values.get("state").map(String::as_str) != Some(self.state.expose())
        {
            return Err(invalid());
        }
        values
            .remove("code")
            .filter(|value| !value.is_empty())
            .map(Secret::new)
            .ok_or_else(invalid)
    }
}

struct Device {
    url: String,
    user_code: String,
    code: Secret,
    interval: Duration,
    expires_at_ms: u64,
}

impl Device {
    fn parse(document: &Value, now: u64) -> Result<Self, Fault> {
        let url = document["verification_uri"]
            .as_str()
            .ok_or_else(invalid)?
            .to_string();
        if !(model_auth::Endpoint {
            url: url.clone(),
            method: model_auth::Method::Get,
            headers: BTreeMap::new(),
            query: BTreeMap::new(),
            body: None,
        })
        .valid()
        {
            return Err(invalid());
        }
        let user_code = document["user_code"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(invalid)?
            .to_string();
        let code = document["device_code"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(invalid)?;
        let seconds = document["expires_in"]
            .as_u64()
            .filter(|value| *value > 0)
            .ok_or_else(invalid)?;
        let expires_at_ms = seconds
            .checked_mul(1000)
            .and_then(|value| now.checked_add(value))
            .ok_or_else(invalid)?;
        let interval = document.get("interval").map_or(Ok(5), |value| {
            value
                .as_u64()
                .filter(|value| *value > 0)
                .ok_or_else(invalid)
        })?;
        Ok(Self {
            url,
            user_code,
            code: Secret::new(code.into()),
            interval: Duration::from_secs(interval),
            expires_at_ms,
        })
    }
}

async fn poll(
    service: &super::super::Service,
    endpoint: &model_auth::Endpoint,
    fields: &BTreeMap<String, Secret>,
    mut interval: Duration,
    expires: u64,
    stop: &CancellationToken,
) -> Result<Value, Fault> {
    loop {
        tokio::select! { biased; _ = stop.cancelled() => return Err(cancelled()), _ = tokio::time::sleep(interval) => {} }
        if super::super::now_ms()? >= expires {
            return Err(super::super::expired());
        }
        let (status, document) = response(service, endpoint, fields, stop).await?;
        match document.get("error").and_then(Value::as_str) {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                interval = interval
                    .checked_add(Duration::from_secs(5))
                    .ok_or_else(invalid)?;
                continue;
            }
            Some("access_denied" | "expired_token" | "invalid_grant") => {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "model authorization failed; sign in again",
                ));
            }
            Some(_) => return Err(invalid()),
            None if status.is_success() => return Ok(document),
            None => return Err(super::super::http_error(status)),
        }
    }
}

fn validate_redirect(value: &str, redirect: model_auth::Redirect) -> Result<(), Fault> {
    match redirect {
        model_auth::Redirect::Loopback => super::super::chatgpt::flow::loopback(value),
        model_auth::Redirect::Paste => {
            let url = Url::parse(value).map_err(|_| invalid())?;
            if url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(invalid());
            }
            Ok(())
        }
    }
}

fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "model authorization cancelled")
}
