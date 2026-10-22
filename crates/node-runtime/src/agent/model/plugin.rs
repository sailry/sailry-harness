//! Pure package translations implement ADK's Llm contract; Node retains transport.
use super::*;
use adk_core::{AdkError, LlmRequest, LlmResponse, LlmResponseStream, SchemaAdapter};
use async_trait::async_trait;
use futures::StreamExt;
use sailry_protocol::plugin::model_adapter::{self, Declaration, Dialect, wire};
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::sync::mpsc;

mod framing;
mod request;
mod response;
#[cfg(test)]
mod tests;
mod transport;
mod worker;
use transport::Credentials;

// A streamed response is retained for its final ADK result. Bound that native
// buffer independently of raw attachments and the VM's separate heap budget.
const RESPONSE_BYTES: usize = 64 * 1024 * 1024;

pub(super) struct Model {
    ingress: Arc<Ingress>,
    provider: Provider,
    config: SessionConfig,
    session: sailry_protocol::SessionId,
    stop: CancellationToken,
    declaration: Declaration,
    bundle: Arc<crate::plugins::script::Bundle>,
    client: reqwest::Client,
}

impl Model {
    pub(super) async fn new(
        ingress: &Arc<Ingress>,
        provider: &Provider,
        config: &SessionConfig,
        session: sailry_protocol::SessionId,
        stop: &CancellationToken,
    ) -> Result<Self, Fault> {
        let (declaration, bundle) = load(ingress, provider, true, stop).await?;
        Ok(Self {
            ingress: ingress.clone(),
            provider: provider.clone(),
            config: config.clone(),
            session,
            stop: stop.clone(),
            declaration,
            bundle: Arc::new(bundle),
            client: transport::client()?,
        })
    }
}

#[async_trait]
impl Llm for Model {
    fn name(&self) -> &str {
        &self.config.model
    }
    fn schema_adapter(&self) -> &dyn SchemaAdapter {
        static GENERIC: adk_core::GenericSchemaAdapter = adk_core::GenericSchemaAdapter;
        static OPENAI: adk_model::openai::OpenAiSchemaAdapter =
            adk_model::openai::OpenAiSchemaAdapter;
        static GEMINI: std::sync::LazyLock<adk_gemini::schema_adapter::GeminiSchemaAdapter> =
            std::sync::LazyLock::new(adk_gemini::schema_adapter::GeminiSchemaAdapter::new);
        match self.declaration.schema_dialect {
            Dialect::Generic => &GENERIC,
            Dialect::OpenAi => &OPENAI,
            Dialect::Gemini => &*GEMINI,
        }
    }

    async fn generate_content(
        &self,
        request: LlmRequest,
        streaming: bool,
    ) -> Result<LlmResponseStream, AdkError> {
        let credentials = credentials(
            &self.ingress,
            &self.provider,
            self.config.credential.as_ref(),
            None,
            &self.stop,
        )
        .await
        .map_err(response::fault)?;
        let mut origin = credentials.origin.clone().unwrap_or_else(|| {
            super::history::origin(&self.provider, &self.config.model, None, None)
        });
        origin.model = self.config.model.clone();
        super::history::generate(request, origin, |request| async move {
            let input = request::convert(
                request,
                &self.provider.endpoint,
                &self.session.to_string(),
                self.config.effort,
                self.config.features.clone(),
                &self.declaration.capabilities,
                streaming,
            )
            .map_err(response::fault)?;
            let names = input.tools.iter().map(|tool| tool.name.clone()).collect();
            let stop = self.stop.child_token();
            // Serialized base64 input is not raw attachment size or VM heap use.
            // The existing VM heap/CPU owner enforces actual execution resources.
            let worker = worker::Worker::new(clone_bundle(&self.bundle), &stop, usize::MAX)
                .await
                .map_err(response::translation)?;
            let mut prepared: wire::Prepared = worker
                .call("prepare", &input)
                .await
                .map_err(response::translation)?;
            let policies = crate::providers::policy::body(&self.provider, &self.config.model);
            if !policies.is_empty() {
                let mut body: serde_json::Value =
                    serde_json::from_str(prepared.body.as_deref().ok_or_else(|| {
                        response::invalid("model request policy requires a JSON body")
                    })?)
                    .map_err(|_| response::invalid("model request policy requires a JSON body"))?;
                let fields = body.as_object_mut().ok_or_else(|| {
                    response::invalid("model request policy requires a JSON object")
                })?;
                for policy in policies {
                    crate::providers::policy::merge(fields, &policy)?;
                }
                prepared.body = Some(
                    serde_json::to_string(&body)
                        .map_err(|_| response::invalid("model request body is invalid"))?,
                );
            }
            let framing = prepared.framing.unwrap_or(self.declaration.framing);
            let received = transport::send(
                &self.client,
                prepared,
                &self.provider.endpoint,
                &self.declaration,
                &credentials,
                &stop,
                usize::MAX,
                self.provider.request_retries(),
            )
            .await
            .map_err(transport::Failure::model)?;
            let retry_at = transport::deadline(received.headers());
            let status = received.status().as_u16();
            let headers =
                transport::headers(received.headers(), &credentials).map_err(response::fault)?;
            let (sender, receiver) = mpsc::channel(1);
            let cancelled = stop.clone();
            tokio::spawn(async move {
                let mut accumulator = response::Accumulator::default();
                let result = produce(
                    received,
                    worker,
                    framing,
                    status,
                    headers,
                    StreamContext {
                        credentials: &credentials,
                        stop: &cancelled,
                        streaming,
                        sender: &sender,
                    },
                    &mut accumulator,
                )
                .await;
                let result =
                    result
                        .and_then(|()| accumulator.finish(&names))
                        .map_err(|mut error| {
                            if let Some(at) = retry_at {
                                error = error.with_retry_deadline(at.into_std());
                            }
                            error
                        });
                let _ = sender.send(result).await;
            });
            let stream: LlmResponseStream = Box::pin(Output { receiver, stop });
            Ok(stream)
        })
        .await
    }
}

struct StreamContext<'a> {
    credentials: &'a Credentials,
    stop: &'a CancellationToken,
    streaming: bool,
    sender: &'a mpsc::Sender<Result<LlmResponse, AdkError>>,
}

async fn produce(
    received: reqwest::Response,
    worker: worker::Worker,
    framing: model_adapter::Framing,
    status: u16,
    headers: std::collections::BTreeMap<String, Vec<String>>,
    context: StreamContext<'_>,
    accumulator: &mut response::Accumulator,
) -> Result<(), AdkError> {
    let StreamContext {
        credentials,
        stop,
        streaming,
        sender,
    } = context;
    let mut bytes = received.bytes_stream();
    let mut decoder = framing::Decoder::new(framing, RESPONSE_BYTES);
    let mut state = serde_json::Value::Null;
    let mut error_body = Vec::new();
    loop {
        let next = tokio::select! {
            _ = stop.cancelled()=>return Err(response::fault(transport::cancelled())),
            next = bytes.next()=>next,
        };
        let done = next.is_none();
        let chunk = next
            .transpose()
            .map_err(|error| transport::Failure::Network(error).model())?;
        if !(200..300).contains(&status) {
            if let Some(chunk) = chunk {
                if error_body
                    .len()
                    .checked_add(chunk.len())
                    .is_none_or(|size| size > RESPONSE_BYTES)
                {
                    return Err(response::resource());
                }
                error_body.extend_from_slice(&chunk);
            }
            if done {
                let body = String::from_utf8(error_body)
                    .map_err(|_| response::invalid("model error response is not UTF-8"))?;
                transport::public(&body, credentials).map_err(response::fault)?;
                #[derive(serde::Deserialize)]
                struct Classified {
                    class: wire::Class,
                    message: String,
                    #[serde(default)]
                    retry_after_ms: Option<u64>,
                    #[serde(default, flatten)]
                    diagnostics: wire::Diagnostics,
                }
                let classified: Classified = if worker.classifies {
                    worker
                        .call(
                            "classify",
                            &serde_json::json!({"status":status,"headers":headers,"body":body}),
                        )
                        .await
                        .map_err(response::translation)?
                } else {
                    Classified {
                        class: response::http_class(status),
                        message: format!("model endpoint returned HTTP {status}"),
                        retry_after_ms: None,
                        diagnostics: Default::default(),
                    }
                };
                return Err(response::diagnostic(
                    response::classified(
                        classified.class,
                        &classified.message,
                        classified.retry_after_ms,
                        Some(status),
                        &credentials.secrets,
                    ),
                    &classified.diagnostics,
                    &credentials.secrets,
                ));
            }
            continue;
        }
        let events = decoder
            .push(chunk.as_deref().unwrap_or_default(), done)
            .map_err(|error| {
                if error.code == ErrorCode::Internal {
                    response::resource()
                } else {
                    response::fault(error)
                }
            })?;
        for event in &events {
            transport::public(&event.data, credentials).map_err(response::fault)?;
        }
        if events.is_empty() && !done {
            continue;
        }
        let decoded: wire::Decoded = worker
            .call(
                "decode",
                &wire::Decode {
                    state,
                    events,
                    done,
                    status: Some(status),
                    headers: Some(headers.clone()),
                },
            )
            .await
            .map_err(response::translation)?;
        state = decoded.state;
        for output in accumulator.push(decoded.deltas, streaming, &credentials.secrets)? {
            if sender.send(Ok(output)).await.is_err() {
                return Err(response::fault(transport::cancelled()));
            }
        }
        if done || accumulator.finished() {
            return Ok(());
        }
    }
}

struct Output {
    receiver: mpsc::Receiver<Result<LlmResponse, AdkError>>,
    stop: CancellationToken,
}
impl futures::Stream for Output {
    type Item = Result<LlmResponse, AdkError>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

async fn load(
    ingress: &Ingress,
    provider: &Provider,
    messages: bool,
    stop: &CancellationToken,
) -> Result<(Declaration, crate::plugins::script::Bundle), Fault> {
    let declaration = ingress.model_adapter(provider, messages).await?;
    let package = provider
        .binding
        .as_ref()
        .and_then(|binding| binding.package.clone())
        .ok_or_else(|| {
            Fault::new(
                ErrorCode::NotConfigured,
                "captured model package is unavailable",
            )
        })?;
    let bundle = read_bundle(&ingress.plugins, package, &declaration, stop).await?;
    Ok((declaration, bundle))
}

async fn read_bundle(
    host: &crate::plugins::Host,
    package: sailry_protocol::plugin::Reference,
    declaration: &Declaration,
    stop: &CancellationToken,
) -> Result<crate::plugins::script::Bundle, Fault> {
    let host = host.clone();
    let script = declaration.script.clone();
    let stop = stop.clone();
    tokio::task::spawn_blocking(move || host.read_translation(&package, &script, stop))
        .await
        .map_err(|_| {
            Fault::new(
                ErrorCode::Unavailable,
                "model translation resource worker failed",
            )
        })?
}

fn clone_bundle(bundle: &crate::plugins::script::Bundle) -> crate::plugins::script::Bundle {
    crate::plugins::script::Bundle {
        entry: bundle.entry.clone(),
        files: bundle.files.clone(),
    }
}

async fn credentials(
    ingress: &Ingress,
    provider: &Provider,
    reference: Option<&sailry_protocol::CredentialRef>,
    key: Option<sailry_protocol::Secret>,
    stop: &CancellationToken,
) -> Result<Credentials, Fault> {
    if provider.authentication == Authentication::ApiKey {
        let (key, credential) = if key.is_some() {
            (key, None)
        } else {
            match reference {
                Some(reference) => {
                    let (credential, key) = ingress
                        .model_credential(reference.clone(), provider.id)
                        .await?;
                    (Some(key), Some(credential))
                }
                None => (None, None),
            }
        };
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(key) = &key {
            let injection = provider
                .binding
                .as_ref()
                .and_then(|binding| binding.key_injection.as_ref());
            let name = injection.map_or("authorization", |injection| injection.header.as_str());
            let template =
                injection.map_or("Bearer {key}", |injection| injection.template.as_str());
            let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| Fault::invalid("model credential header is invalid"))?;
            let mut value =
                reqwest::header::HeaderValue::from_str(&template.replace("{key}", key.expose()))
                    .map_err(|_| Fault::invalid("model credential header is invalid"))?;
            value.set_sensitive(true);
            headers
                .try_insert(name, value)
                .map_err(|_| Fault::invalid("model credential header is invalid"))?;
        }
        Ok(Credentials {
            origin: Some(super::history::origin(
                provider,
                "",
                credential.as_ref(),
                None,
            )),
            headers,
            access: None,
            secrets: key.clone().into_iter().collect(),
            key,
        })
    } else {
        let reference = reference
            .cloned()
            .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "provider sign-in is required"))?;
        let options = crate::providers::login::effective(provider)?;
        let authorized = ingress
            .authorization(
                reference,
                provider.id,
                provider.authentication,
                &options,
                stop,
            )
            .await?;
        let origin = super::history::origin(
            provider,
            "",
            Some(&authorized.credential),
            Some(&authorized.grant),
        );
        let grant = authorized.grant;
        let headers = grant.headers(&options)?;
        let secrets = grant.secrets(&headers);
        Ok(Credentials {
            origin: Some(origin),
            headers,
            access: Some(grant.access().clone()),
            key: None,
            secrets,
        })
    }
}

/// Discovery uses the same captured resources and credential boundary, without
/// exposing conversation data or making an implicit second HTTP request.
pub(crate) async fn discover(
    ingress: &Ingress,
    provider: &Provider,
    key: Option<sailry_protocol::Secret>,
    stop: &CancellationToken,
) -> Result<sailry_protocol::conversation::discovery::Catalog, Fault> {
    let stop = stop.child_token();
    let _guard = stop.clone().drop_guard();
    tokio::select! {
        _ = stop.cancelled()=>Err(transport::cancelled()),
        result = tokio::time::timeout(std::time::Duration::from_secs(30),discover_inner(ingress,provider,key,&stop))=>result.map_err(|_|Fault::new(ErrorCode::Unavailable,"model discovery deadline exceeded"))?,
    }
}

async fn discover_inner(
    ingress: &Ingress,
    provider: &Provider,
    key: Option<sailry_protocol::Secret>,
    stop: &CancellationToken,
) -> Result<sailry_protocol::conversation::discovery::Catalog, Fault> {
    let (declaration, bundle) = load(ingress, provider, false, stop).await?;
    if !declaration.discovery {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "channel plugin does not declare model discovery",
        ));
    }
    let credentials =
        credentials(ingress, provider, provider.credential.as_ref(), key, stop).await?;
    read_catalog(provider, &declaration, bundle, credentials, stop).await
}

/// Login already owns the newly issued grant; discovery must use the same
/// captured adapter before credential persistence, without resolving another grant.
pub(crate) async fn discover_authorized(
    host: &crate::plugins::Host,
    provider: &Provider,
    declaration: &Declaration,
    grant: &crate::providers::login::Grant,
    options: &sailry_protocol::conversation::oauth::Options,
    stop: &CancellationToken,
) -> Result<sailry_protocol::conversation::discovery::Catalog, Fault> {
    let stop = stop.child_token();
    let _guard = stop.clone().drop_guard();
    let operation = async {
        if !declaration.discovery {
            return Err(Fault::new(
                ErrorCode::NotConfigured,
                "channel plugin does not declare model discovery",
            ));
        }
        let package = provider
            .binding
            .as_ref()
            .and_then(|binding| binding.package.clone())
            .ok_or_else(|| {
                Fault::new(
                    ErrorCode::NotConfigured,
                    "captured model package is unavailable",
                )
            })?;
        let bundle = read_bundle(host, package, declaration, &stop).await?;
        let headers = grant.headers(options)?;
        let secrets = grant.secrets(&headers);
        let credentials = Credentials {
            origin: None,
            headers,
            access: Some(grant.access().clone()),
            key: None,
            secrets,
        };
        read_catalog(provider, declaration, bundle, credentials, &stop).await
    };
    tokio::select! {
        _ = stop.cancelled()=>Err(transport::cancelled()),
        result = tokio::time::timeout(std::time::Duration::from_secs(30),operation)=>result.map_err(|_|Fault::new(ErrorCode::Unavailable,"model discovery deadline exceeded"))?,
    }
}

async fn read_catalog(
    provider: &Provider,
    declaration: &Declaration,
    bundle: crate::plugins::script::Bundle,
    credentials: Credentials,
    stop: &CancellationToken,
) -> Result<sailry_protocol::conversation::discovery::Catalog, Fault> {
    let mut stage = "prepare";
    let mut upstream_status = None;
    let result = async {
        const DISCOVERY_BYTES: usize = 8 * 1024 * 1024;
        let worker = worker::Worker::new(bundle, stop, DISCOVERY_BYTES).await?;
        let prepared: wire::Prepared = worker
            .call(
                "prepare_discovery",
                &serde_json::json!({"endpoint":provider.endpoint}),
            )
            .await?;
        stage = "request";
        let received = transport::send(
            &transport::client()?,
            prepared,
            &provider.endpoint,
            declaration,
            &credentials,
            stop,
            DISCOVERY_BYTES,
            0,
        )
        .await.map_err(transport::Failure::fault)?;
        let status = received.status().as_u16();
        upstream_status = Some(u64::from(status));
        stage = "headers";
        let headers = transport::headers(received.headers(), &credentials)?;
        stage = "body";
        let mut chunks = received.bytes_stream();
        let mut body = Vec::new();
        loop {
            let chunk = tokio::select! { _=stop.cancelled()=>return Err(transport::cancelled()),chunk=chunks.next()=>chunk };
            let Some(chunk) = chunk else {
                break;
            };
            let chunk = chunk.map_err(|_| transport::network())?;
            if body
                .len()
                .checked_add(chunk.len())
                .is_none_or(|size| size > DISCOVERY_BYTES)
            {
                return Err(Fault::new(
                    ErrorCode::Internal,
                    "model adapter resource budget exceeded",
                ));
            }
            body.extend_from_slice(&chunk);
        }
        stage = "status";
        if !(200..300).contains(&status) {
            // Failed discovery bodies are never sent to the VM or diagnostics.
            return Err(Fault::new(
                ErrorCode::Unavailable,
                format!("model discovery failed with HTTP {status}"),
            ));
        }
        let body = String::from_utf8(body)
            .map_err(|_| Fault::invalid("model discovery response is not UTF-8"))?;
        stage = "privacy";
        transport::public(&body, &credentials)?;
        stage = "decode";
        let mut catalog:sailry_protocol::conversation::discovery::Catalog = worker.call("decode_discovery",&serde_json::json!({"endpoint":provider.endpoint,"status":status,"headers":headers,"body":body})).await?;
        if catalog.endpoint.trim_end_matches('/') != provider.endpoint.trim_end_matches('/') {
            return Err(Fault::invalid(
                "model discovery returned a different endpoint",
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        for model in &mut catalog.models {
            if model.id.trim().is_empty()
                || !ids.insert(model.id.clone())
                || model.id.chars().any(char::is_control)
                || model.context == Some(0)
                || model.output == Some(0)
                || model
                    .context
                    .zip(model.output)
                    .is_some_and(|(context, output)| output > context)
            {
                return Err(Fault::invalid("model discovery returned invalid metadata"));
            }
            // Catalog summaries cannot elevate the captured transport's abilities.
            let capabilities = model.capabilities.get_or_insert_with(Default::default);
            sailry_protocol::conversation::features::validate(&capabilities.features)?;
            if !declaration.capabilities.vision {
                capabilities.vision = Some(false);
            }
            if !declaration.capabilities.tools {
                capabilities.tools = Some(false);
            }
            if !declaration.capabilities.reasoning {
                capabilities.reasoning = Some(false);
            }
        }
        Ok(catalog)
    }.await;
    if let Err(error) = &result {
        // Only typed failure metadata is logged, never bodies, headers or grant fields.
        let code = serde_json::to_value(error.code)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        if let Some(status) = upstream_status {
            tracing::warn!(target: "sailry", area="model", phase="failed", operation="model_discovery", stage, code=code.as_str(), upstream_status=status);
        } else {
            tracing::warn!(target: "sailry", area="model", phase="failed", operation="model_discovery", stage, code=code.as_str());
        }
    }
    result
}
