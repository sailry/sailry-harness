//! Package steps compose the existing private login services, without installing an SDK in the VM.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sailry_protocol::conversation::login::State;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{future::Future, time::Duration};
use tokio::sync::oneshot;

pub(crate) type IssuedClient = (String, oneshot::Sender<Result<(), Fault>>);

#[derive(Default)]
pub(crate) struct Context {
    pub previous: Option<super::super::Grant>,
    pub host_id: String,
    pub registration: Option<String>,
    pub issued: Option<oneshot::Sender<IssuedClient>>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Effect {
    Request {
        name: String,
        fields: BTreeMap<String, Secret>,
        state: Value,
    },
    Prompt {
        url: String,
        user_code: Option<String>,
        expires_at_ms: u64,
        state: Value,
    },
    Callback {
        proof: Proof,
        allow_code: bool,
        state: Value,
    },
    Wait {
        delay_ms: u64,
        state: Value,
    },
    Verify {
        token: Secret,
        audience: String,
        nonce: Option<String>,
        state: Value,
    },
    Registered {
        client_id: String,
        state: Value,
    },
    Finish {
        grant: Value,
    },
    Reject {
        code: ErrorCode,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Proof {
    State,
    Verifier,
}

pub(super) async fn run<P, F>(
    input: &mut flow::Input,
    previous: Option<Value>,
    progress: P,
    stop: CancellationToken,
) -> Result<Value, Fault>
where
    P: FnOnce(State) -> F,
    F: Future<Output = Result<(), Fault>>,
{
    let model_auth::Flow::Script {
        handler,
        requests,
        callback,
        issuer,
        ..
    } = &input.declaration.flow
    else {
        return Err(invalid());
    };
    let bundle = input.exchange.take().ok_or_else(invalid)?;
    let stop = stop.child_token();
    let _cancel = stop.clone().drop_guard();
    let mut context = input.context.take().unwrap_or_default();
    let refreshing = previous.is_some();
    let previous = previous.or(context
        .previous
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|_| invalid())?);
    let verifier = super::super::random();
    let state_proof = super::super::random();
    let nonce = super::super::random();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.expose().as_bytes()));
    input
        .fields
        .entry("user_agent".into())
        .or_insert_with(|| Secret::new("Sailry/0.1".into()));
    let initial = json!({
        "previous":previous,"host_id":context.host_id,"registration":context.registration,
        "redirect":input.redirect,"fields":input.fields,"scopes":input.declaration.scopes,
        "proof":{"verifier":verifier,"state":state_proof,"nonce":nonce,"challenge":challenge},
    });
    let mut event = json!({"kind":if refreshing {"refresh"} else {"start"}});
    let mut state = Value::Null;
    let mut progress = Some(progress);
    let mut expires = super::super::now_ms()? + super::super::DEADLINE.as_millis() as u64;
    let mut deadline = tokio::time::Instant::now() + super::super::DEADLINE;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(super::super::expired());
        }
        let action_deadline = deadline;
        let step =
            json!({"context":initial,"now_ms":super::super::now_ms()?,"event":event,"state":state});
        let handler = handler.clone();
        let cloned = crate::plugins::script::Bundle {
            entry: bundle.entry.clone(),
            files: bundle.files.clone(),
        };
        let interrupted = stop.clone();
        let transformed = tokio::task::spawn_blocking(move || {
            let mut session =
                crate::plugins::script::translation::Session::new(cloned, interrupted)
                    .inspect_err(|error| {
                        diagnostic("script", "session_create", Some(error.code), None)
                    })?;
            // A step includes previous credentials, a bounded HTTP document and
            // its private continuation, rather than just one response body.
            session
                .call(&handler, &step, super::super::BODY_LIMIT * 4)
                .inspect_err(|error| diagnostic("script", "handler_call", Some(error.code), None))
        });
        let effect: Effect = tokio::select! {
            biased;
            _=stop.cancelled()=>return Err(cancelled()),
            result=tokio::time::timeout_at(deadline,transformed)=>{
                let value = result.map_err(|_| {
                    diagnostic("script", "step_deadline", Some(ErrorCode::Expired), None);
                    super::super::expired()
                })?.map_err(|_| {
                    diagnostic("script", "worker_join", Some(ErrorCode::InvalidRequest), None);
                    invalid()
                })??;
                serde_json::from_value(value).map_err(|_| {
                    diagnostic("effect", "effect_decode", Some(ErrorCode::InvalidRequest), None);
                    invalid()
                })?
            },
        };
        let action = async {
            match effect {
                Effect::Request {
                    name,
                    fields,
                    state: next,
                } => {
                    let stage = request_stage(&name);
                    let endpoint = requests.get(&name).ok_or_else(invalid)?;
                    for key in ["client_id", "client_secret"] {
                        if let (Some(configured), Some(requested)) =
                            (input.fields.get(key), fields.get(key))
                            && configured != requested
                        {
                            return Err(invalid());
                        }
                    }
                    let mut resolved = input.fields.clone();
                    resolved.extend(fields);
                    let (status, body) = super::response_with(
                        &input.service,
                        endpoint,
                        &resolved,
                        &stop,
                        refreshing,
                        true,
                    )
                    .await
                    .inspect_err(|error| {
                        diagnostic(stage, "request_failed", Some(error.code), None)
                    })?;
                    diagnostic(
                        stage,
                        if status.is_success() {
                            "http_success"
                        } else {
                            "http_rejected"
                        },
                        None,
                        Some(status),
                    );
                    state = next;
                    event = json!({"kind":"response","status":status.as_u16(),"body":body});
                }
                Effect::Prompt {
                    url,
                    user_code,
                    expires_at_ms,
                    state: next,
                } => {
                    if !(model_auth::Endpoint {
                        cache_ttl_ms: None,
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
                    // Match the existing authorization fixture's browser origin,
                    // while the captured declaration retains its production URL.
                    #[cfg(any(test, feature = "test-support"))]
                    let url = if user_code.is_none()
                        && let Some(endpoint) = &input.service.model_endpoint
                    {
                        let source = Url::parse(&url).map_err(|_| invalid())?;
                        let mut routed = Url::parse(endpoint).map_err(|_| invalid())?;
                        routed.set_path(source.path());
                        routed.set_query(source.query());
                        routed.into()
                    } else {
                        url
                    };
                    expires = expires.min(expires_at_ms);
                    let duration = expires
                        .checked_sub(super::super::now_ms()?)
                        .filter(|duration| *duration > 0)
                        .ok_or_else(super::super::expired)?;
                    deadline =
                        deadline.min(tokio::time::Instant::now() + Duration::from_millis(duration));
                    let prompt = match user_code {
                        Some(user_code)
                            if !user_code.is_empty()
                                && user_code.len() <= 256
                                && !user_code.chars().any(char::is_control) =>
                        {
                            State::Pending {
                                verification_url: url,
                                user_code,
                                expires_at_ms: expires,
                            }
                        }
                        None => State::Browser {
                            authorization_url: url,
                            expires_at_ms: expires,
                        },
                        _ => return Err(invalid()),
                    };
                    progress.take().ok_or_else(invalid)?(prompt).await?;
                    state = next;
                    event = json!({"kind":"presented"});
                }
                Effect::Callback {
                    proof,
                    allow_code,
                    state: next,
                } => {
                    let declaration = callback.as_ref().ok_or_else(invalid)?;
                    if allow_code && !declaration.manual {
                        return Err(invalid());
                    }
                    let redirect = input.redirect.as_deref().ok_or_else(invalid)?;
                    validate_redirect(redirect, declaration)?;
                    let returned = input
                        .callback
                        .take()
                        .ok_or_else(invalid)?
                        .await
                        .map_err(|_| cancelled())?;
                    let expected = match proof {
                        Proof::State => state_proof.expose(),
                        Proof::Verifier => verifier.expose(),
                    };
                    let fields = callback_fields(&returned, redirect, expected, allow_code)?;
                    state = next;
                    event = json!({"kind":"callback","fields":fields});
                }
                Effect::Wait {
                    delay_ms,
                    state: next,
                } => {
                    if delay_ms == 0 {
                        return Err(invalid());
                    }
                    tokio::time::sleep(Duration::from_millis(delay_ms).min(remaining)).await;
                    state = next;
                    event = json!({"kind":"elapsed"});
                }
                Effect::Verify {
                    token,
                    audience,
                    nonce,
                    state: next,
                } => {
                    diagnostic("identity", "started", None, None);
                    let issuer = issuer.as_deref().ok_or_else(invalid)?;
                    let client = super::super::http_client(
                        input
                            .fields
                            .get("user_agent")
                            .map_or("Sailry/0.1", Secret::expose),
                    )
                    .inspect_err(|error| {
                        diagnostic("identity", "client_create", Some(error.code), None)
                    })?;
                    #[cfg(any(test, feature = "test-support"))]
                    let issuer = input.service.model_endpoint.as_deref().unwrap_or(issuer);
                    let claims = super::identity::verify(
                        &client,
                        issuer,
                        &token,
                        &audience,
                        nonce.as_deref(),
                    )
                    .await?;
                    state = next;
                    event = json!({"kind":"identity","claims":claims});
                }
                Effect::Registered {
                    client_id,
                    state: next,
                } => {
                    if client_id.is_empty()
                        || client_id.len() > 256
                        || !client_id
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                    {
                        return Err(invalid());
                    }
                    let (reply, received) = oneshot::channel();
                    context
                        .issued
                        .take()
                        .ok_or_else(invalid)?
                        .send((client_id, reply))
                        .map_err(|_| cancelled())?;
                    received.await.map_err(|_| cancelled())??;
                    state = next;
                    event = json!({"kind":"registered"});
                }
                Effect::Finish { grant } => {
                    diagnostic("grant", "received", None, None);
                    return Ok(Some(grant));
                }
                Effect::Reject { code } => {
                    diagnostic("effect", "rejected", Some(code), None);
                    return Err(match code {
                        ErrorCode::Expired => super::super::expired(),
                        ErrorCode::PermissionDenied => {
                            Fault::new(code, "provider authorization was not granted")
                        }
                        ErrorCode::OutcomeUnknown => uncertain(),
                        _ => invalid(),
                    });
                }
            }
            Ok(None)
        };
        let result = tokio::select! {
            biased;
            _=stop.cancelled()=>return Err(cancelled()),
            result=tokio::time::timeout_at(action_deadline,action)=>result.map_err(|_|super::super::expired())??,
        };
        if let Some(grant) = result {
            return Ok(grant);
        }
    }
}

pub(crate) fn validate_redirect(
    value: &str,
    declaration: &model_auth::Callback,
) -> Result<(), Fault> {
    if declaration.port.is_none()
        && declaration.manual
        && declaration.redirect_uri.as_deref() == Some(value)
    {
        return Ok(());
    }
    let url = Url::parse(value).map_err(|_| invalid())?;
    if url.scheme() != "http"
        || !(url.host_str() == Some("127.0.0.1")
            || declaration.allow_localhost && url.host_str() == Some("localhost"))
        || url.port().is_none_or(|port| port == 0)
        || declaration
            .port
            .is_some_and(|port| url.port() != Some(port))
        || declaration.port.is_some()
            && declaration
                .redirect_uri
                .as_deref()
                .is_some_and(|uri| uri != value)
        || url.path() != declaration.path
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.as_str() != value
    {
        return Err(invalid());
    }
    Ok(())
}

fn callback_fields(
    input: &Secret,
    redirect: &str,
    expected: &str,
    allow_code: bool,
) -> Result<BTreeMap<String, String>, Fault> {
    let value = input.expose().trim();
    let mut fields = BTreeMap::new();
    if value.starts_with("http://") || value.starts_with("https://") {
        let mut url = Url::parse(value).map_err(|_| invalid())?;
        if url.fragment().is_some() {
            return Err(invalid());
        }
        for (name, value) in url.query_pairs() {
            if fields
                .insert(name.into_owned(), value.into_owned())
                .is_some()
            {
                return Err(invalid());
            }
        }
        url.set_query(None);
        if url.as_str() != redirect {
            return Err(invalid());
        }
    } else if allow_code {
        if value.contains("code=") {
            for (name, value) in url::form_urlencoded::parse(value.as_bytes()) {
                if fields
                    .insert(name.into_owned(), value.into_owned())
                    .is_some()
                {
                    return Err(invalid());
                }
            }
        } else {
            let (code, state) = value
                .split_once('#')
                .map_or((value, None), |(code, state)| (code, Some(state)));
            fields.insert("code".into(), code.into());
            if let Some(state) = state {
                fields.insert("state".into(), state.into());
            }
        }
    } else {
        return Err(invalid());
    }
    if fields.get("state").map(String::as_str) != Some(expected)
        && (!allow_code || fields.contains_key("state"))
    {
        return Err(invalid());
    }
    if fields.contains_key("error") {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "provider authorization was not granted",
        ));
    }
    if fields.get("code").is_none_or(|code| {
        code.is_empty()
            || code.len() > 16384
            || code.chars().any(char::is_whitespace)
            || code.chars().any(char::is_control)
    }) {
        return Err(invalid());
    }
    Ok(fields)
}

fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "provider authorization cancelled")
}

fn request_stage(name: &str) -> &'static str {
    match name {
        "device" => "device",
        "poll" => "poll",
        "token" => "token",
        "refresh" => "refresh",
        _ => "http",
    }
}

pub(super) fn diagnostic(
    stage: &'static str,
    reason: &'static str,
    code: Option<ErrorCode>,
    status: Option<reqwest::StatusCode>,
) {
    let mut entry = crate::logging::Entry::new(
        crate::logging::Area::Node,
        if code.is_some() {
            crate::logging::Phase::Failed
        } else {
            crate::logging::Phase::Completed
        },
    );
    entry.operation = Some("provider_authorization");
    entry.source_kind = Some("model_auth");
    entry.stage = Some(stage);
    entry.reason = Some(reason);
    entry.code = code;
    entry.upstream_status = status.map(|status| u64::from(status.as_u16()));
    if code.is_some() {
        entry.emit()
    } else {
        entry.emit_debug()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enforces_fixed_port_for_manual_callbacks() {
        let callback = model_auth::Callback {
            path: "/auth/callback".into(),
            port: Some(1455),
            redirect_uri: Some("http://localhost:1455/auth/callback".into()),
            manual: true,
            allow_localhost: true,
        };
        assert!(validate_redirect(callback.redirect_uri.as_ref().unwrap(), &callback).is_ok());
        for value in [
            "http://localhost:1456/auth/callback",
            "http://127.0.0.1:1455/auth/callback",
            "http://remote.test:1455/auth/callback",
            "http://localhost:1455/wrong",
            "http://localhost:1455/auth/callback?code=value",
        ] {
            assert!(validate_redirect(value, &callback).is_err(), "{value}");
        }
        let invalid = model_auth::Callback {
            redirect_uri: Some("http://localhost:1456/auth/callback".into()),
            ..callback
        };
        assert!(validate_redirect(invalid.redirect_uri.as_ref().unwrap(), &invalid).is_err());
    }
    #[test]
    fn binds_callbacks_to_proof_and_redirect() {
        let redirect = "http://127.0.0.1:12345/callback";
        let returned = Secret::new(format!(
            "{redirect}?code=private-code&state=proof&client_id=issued"
        ));
        let fields = callback_fields(&returned, redirect, "proof", false).unwrap();
        assert_eq!(fields["client_id"], "issued");
        assert!(callback_fields(&returned, redirect, "other", false).is_err());
        assert!(
            callback_fields(&returned, "http://127.0.0.1:12346/callback", "proof", false).is_err()
        );
        assert!(
            callback_fields(
                &Secret::new(format!("{}/x?code=code&state=proof", redirect)),
                redirect,
                "proof",
                false
            )
            .is_err()
        );
        assert!(
            callback_fields(
                &Secret::new(format!("{redirect}?code=code&state=proof&state=proof")),
                redirect,
                "proof",
                false
            )
            .is_err()
        );
    }
    #[test]
    fn accepts_declared_pasted_codes() {
        for value in [
            "private-code",
            "private-code#proof",
            "code=private-code&state=proof",
        ] {
            assert_eq!(
                callback_fields(
                    &Secret::new(value.into()),
                    "https://example.test/callback",
                    "proof",
                    true
                )
                .unwrap()["code"],
                "private-code"
            );
            assert!(
                callback_fields(
                    &Secret::new(value.into()),
                    "https://example.test/callback",
                    "proof",
                    false
                )
                .is_err()
            );
        }
        assert!(
            callback_fields(
                &Secret::new("private-code#wrong".into()),
                "https://example.test/callback",
                "proof",
                true
            )
            .is_err()
        );
    }
    #[test]
    fn validates_declared_loopback_path() {
        let callback = model_auth::Callback {
            path: "/auth/callback".into(),
            port: None,
            redirect_uri: Some("https://example.test/callback".into()),
            manual: true,
            allow_localhost: true,
        };
        assert!(validate_redirect("http://127.0.0.1:12345/auth/callback", &callback).is_ok());
        assert!(validate_redirect("https://example.test/callback", &callback).is_ok());
        for value in [
            "http://127.0.0.1:12345/wrong",
            "http://remote.test:12345/auth/callback",
            "http://127.0.0.1:12345/auth/callback?state=value",
        ] {
            assert!(validate_redirect(value, &callback).is_err());
        }
    }
}
