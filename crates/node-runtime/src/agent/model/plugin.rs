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
        let input = request::convert(
            request,
            &self.provider.endpoint,
            &self.session.to_string(),
            self.config.effort,
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
            let fields = body
                .as_object_mut()
                .ok_or_else(|| response::invalid("model request policy requires a JSON object"))?;
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
        )
        .await
        .map_err(response::fault)?;
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
            let result = result.and_then(|()| accumulator.finish(&names));
            let _ = sender.send(result).await;
        });
        Ok(Box::pin(Output { receiver, stop }))
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
        let chunk = next.transpose().map_err(|_| {
            let mut error = response::fault(transport::network());
            error.retry.should_retry &= !accumulator.published();
            error
        })?;
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
                }
                let classified: Classified = worker
                    .call(
                        "classify",
                        &serde_json::json!({"status":status,"headers":headers,"body":body}),
                    )
                    .await
                    .map_err(response::translation)?;
                return Err(response::classified(
                    classified.class,
                    &classified.message,
                    classified.retry_after_ms,
                    Some(status),
                    false,
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
        // Activity events keep the existing outer idle timeout alive even when
        // an SSE heartbeat contains no model delta. No user text is fabricated.
        if events.is_empty() && !done {
            if streaming
                && sender
                    .send(Ok(LlmResponse {
                        partial: true,
                        ..Default::default()
                    }))
                    .await
                    .is_err()
            {
                return Err(response::fault(transport::cancelled()));
            }
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
        if done {
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
    let host = ingress.plugins.clone();
    let script = declaration.script.clone();
    let stop = stop.clone();
    let bundle =
        tokio::task::spawn_blocking(move || host.read_translation(&package, &script, stop))
            .await
            .map_err(|_| {
                Fault::new(
                    ErrorCode::Unavailable,
                    "model translation resource worker failed",
                )
            })??;
    Ok((declaration, bundle))
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
        let key = if key.is_some() {
            key
        } else {
            match reference {
                Some(reference) => Some(
                    ingress
                        .resolve_credential(reference.clone(), provider.id)
                        .await?,
                ),
                None => None,
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
        let grant = ingress
            .authorization(
                reference,
                provider.id,
                provider.authentication,
                &options,
                stop,
            )
            .await?
            .grant;
        let headers = grant.headers(&options)?;
        let secrets = super::authorization::credentials(&grant, &headers);
        Ok(Credentials {
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
    const DISCOVERY_BYTES: usize = 8 * 1024 * 1024;
    let credentials =
        credentials(ingress, provider, provider.credential.as_ref(), key, stop).await?;
    let worker = worker::Worker::new(bundle, stop, DISCOVERY_BYTES).await?;
    let prepared: wire::Prepared = worker
        .call(
            "prepare_discovery",
            &serde_json::json!({"endpoint":provider.endpoint}),
        )
        .await?;
    let received = transport::send(
        &transport::client()?,
        prepared,
        &provider.endpoint,
        &declaration,
        &credentials,
        stop,
        DISCOVERY_BYTES,
    )
    .await?;
    let status = received.status().as_u16();
    let headers = transport::headers(received.headers(), &credentials)?;
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
    let body = String::from_utf8(body)
        .map_err(|_| Fault::invalid("model discovery response is not UTF-8"))?;
    transport::public(&body, &credentials)?;
    if !(200..300).contains(&status) {
        // A decoder may classify failures, but unchecked HTTP bodies and headers
        // must never become discovery diagnostics or public model metadata.
        return Err(Fault::new(
            ErrorCode::Unavailable,
            format!("model discovery failed with HTTP {status}"),
        ));
    }
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
}
