//! Generation stays on the admitted route, without replay or account fallback.
use super::*;
use crate::providers::{
    media::{ImageRoute, image_api, image_route, supports_api},
    request::{self as authorization, Metadata},
};
use reqwest::{
    Client, Response,
    header::{HeaderMap, HeaderValue, USER_AGENT},
};
use sailry_protocol::{RequestId, Secret};
mod image;
mod response;

const IMAGE_BYTES: usize = 64 * 1024 * 1024;
const VIDEO_BYTES: usize = 512 * 1024 * 1024;

enum Generated {
    File {
        bytes: Vec<u8>,
        job: Option<String>,
        usage: Option<Value>,
    },
    Failed {
        job: String,
    },
}

/// One authorized request target, prepared by the same Node owner as model requests.
struct Target {
    base: reqwest::Url,
    headers: HeaderMap,
    prepared: authorization::Prepared,
    provider: sailry_protocol::conversation::Provider,
}

impl Target {
    fn url(&self, suffix: &str) -> reqwest::Url {
        let mut url = self.base.clone();
        let path = format!("{}/{suffix}", url.path().trim_end_matches('/'));
        url.set_path(&path);
        url
    }

    fn payload(
        &self,
        route: Option<ImageRoute>,
        model: &str,
        prompt: &str,
    ) -> Result<(&'static str, Value), Fault> {
        let (suffix, mut payload) = image::request(route, model, prompt);
        let responses = matches!(route, Some(ImageRoute::Responses | ImageRoute::Authorized));
        if responses {
            // The Responses route shares the chat model's API contract and request policy.
            for policy in crate::providers::policy::body(&self.provider, model) {
                for (key, value) in policy {
                    if !matches!(
                        key.as_str(),
                        "model"
                            | "input"
                            | "tools"
                            | "tool_choice"
                            | "max_tool_calls"
                            | "store"
                            | "include"
                    ) {
                        payload[&key] = value;
                    }
                }
            }
            self.prepared.apply_json(&mut payload)?;
            if route == Some(ImageRoute::Authorized) {
                payload["stream"] = json!(true);
                payload["store"] = json!(false);
            }
        }
        Ok((suffix, payload))
    }
}

impl Media {
    pub(super) async fn generate(&self, prompt: String, path: String) -> Result<Value, Failure> {
        let route = if self.kind == Kind::Image {
            let metadata = self
                .model
                .provider
                .models
                .iter()
                .find(|model| model.id == self.model.model);
            Some(
                metadata
                    .map_or_else(
                        || image_api(&self.model.provider),
                        |model| image_route(&self.model.provider, model),
                    )
                    .ok_or_else(Failure::unsupported)?,
            )
        } else {
            if !supports_api(Kind::Video, &self.model.provider) {
                return Err(Failure::unsupported());
            }
            None
        };
        // This route does not expose native generated image blocks.
        if route == Some(ImageRoute::Native) {
            return Err(Failure::unsupported());
        }
        let root = self.ingress.worktree_root(self.worktree).await?;
        let limit = if self.kind == Kind::Image {
            IMAGE_BYTES
        } else {
            VIDEO_BYTES
        };
        let output = self
            .ingress
            .files
            .prepare_media(root, path, limit as u64)
            .await?;
        let generated = self.http(prompt, route).await?;
        let (bytes, job, reported_usage) = match generated {
            Generated::File { bytes, job, usage } => (bytes, job, usage),
            Generated::Failed { job } => {
                return Ok(
                    json!({"job":job,"error":Fault::new(ErrorCode::Unavailable,"video generation failed")}),
                );
            }
        };
        if self.kind == Kind::Image
            && file_format::FileFormat::from_bytes(&bytes)
                != file_format::FileFormat::PortableNetworkGraphics
        {
            return Err(unavailable(
                "image generation returned an unsupported format; PNG required",
            )
            .into());
        }
        if self.kind == Kind::Video
            && !matches!(
                file_format::FileFormat::from_bytes(&bytes),
                file_format::FileFormat::Mpeg4Part14 | file_format::FileFormat::Mpeg4Part14Video
            )
        {
            return Err(unavailable(
                "video generation returned an unsupported format; MP4 required",
            )
            .into());
        }
        let path = self
            .ingress
            .files
            .publish_media(output, bytes, self.stop.clone())
            .await?;
        Ok(json!({"path":path,"job":job,"usage":reported_usage}))
    }

    /// Resolve the turn's captured provider and account through the Node credential owner.
    async fn target(&self) -> Result<Target, Failure> {
        let config = vision::configuration(&self.config, &self.model);
        let resolved = authorization::current(
            &self.ingress,
            &self.model.provider,
            config.credential.as_ref(),
            &self.stop,
        )
        .await?;
        let prepared = authorization::prepare(
            &resolved.provider,
            &resolved.credentials,
            Metadata {
                model: &self.model.model,
                session: self
                    .session
                    .parse()
                    .map_err(|_| Fault::invalid("invalid media session"))?,
                request: RequestId::new(),
                system: "",
                tool_names: &[],
                delegated: true,
                user: false,
                vision: false,
            },
        )?;
        let mut headers = prepared.headers.clone();
        // Account authorizations supply their own client identity.
        headers
            .entry(USER_AGENT)
            .or_insert(HeaderValue::from_static(concat!(
                "Sailry/",
                env!("CARGO_PKG_VERSION")
            )));
        Ok(Target {
            base: base(&prepared.endpoint)?,
            headers,
            prepared,
            provider: resolved.provider,
        })
    }

    async fn http(&self, prompt: String, route: Option<ImageRoute>) -> Result<Generated, Failure> {
        let target = self.target().await?;
        let secrets = target.prepared.secrets.clone();
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(240))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| unavailable("media HTTP client unavailable"))?;
        let (suffix, payload) = target.payload(route, &self.model.model, &prompt)?;
        let responses = matches!(route, Some(ImageRoute::Responses | ImageRoute::Authorized));
        let mut request = client
            .post(target.url(suffix))
            .headers(target.headers.clone())
            .json(&payload);
        if payload["stream"] == true {
            request = request.header(reqwest::header::ACCEPT, "text/event-stream");
        }
        let response = request.send().await.map_err(|_| uncertain())?;
        let value = if responses {
            let status = response.status();
            let value = response::read(response, IMAGE_BYTES * 2, &secrets).await;
            if status == reqwest::StatusCode::UNAUTHORIZED
                && let Some(fault) = self
                    .ingress
                    .authorization_failure(
                        &self.model.provider,
                        target.provider.credential.as_ref(),
                    )
                    .await?
            {
                return Err(fault.into());
            }
            value?
        } else {
            let response_bytes = body(
                response,
                if self.kind == Kind::Image {
                    IMAGE_BYTES * 2
                } else {
                    1024 * 1024
                },
                true,
                &secrets,
            )
            .await?;
            serde_json::from_slice(&response_bytes)
                .map_err(|_| unavailable("invalid media generation response"))?
        };
        let mut reported_usage = value.get("usage").cloned();
        let (bytes, job) = if self.kind == Kind::Image {
            self.record_usage(usage::generation(reported_usage.as_ref()), None)
                .await?;
            let bytes = if responses {
                image::completed(&value, &secrets)?
            } else if let Some(encoded) = value.pointer("/data/0/b64_json").and_then(Value::as_str)
            {
                image::decode(encoded)?
            } else if let Some(url) = value.pointer("/data/0/url").and_then(Value::as_str) {
                let url = reqwest::Url::parse(url)
                    .map_err(|_| unavailable("invalid generated image URL"))?;
                if !matches!(url.scheme(), "http" | "https")
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    return Err(unavailable("invalid generated image URL").into());
                }
                body(
                    client
                        .get(url)
                        .send()
                        .await
                        .map_err(|_| unavailable("generated image download failed"))?,
                    IMAGE_BYTES,
                    false,
                    &secrets,
                )
                .await?
            } else {
                return Err(unavailable("generation returned no image content").into());
            };
            (bytes, None)
        } else {
            let id = value
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| {
                    !id.is_empty()
                        && id.len() <= 256
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
                })
                .ok_or_else(|| unavailable("video generation returned an invalid job ID"))?;
            let mut current = value.clone();
            loop {
                match current.get("status").and_then(Value::as_str) {
                    Some("completed") => break,
                    Some("queued" | "in_progress") => {}
                    Some("failed" | "cancelled") => {
                        return Ok(Generated::Failed { job: id.to_owned() });
                    }
                    _ => {
                        return Err(
                            unavailable("video generation returned an invalid status").into()
                        );
                    }
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
                let request = client
                    .get(target.url(&format!("videos/{id}")))
                    .headers(target.headers.clone());
                let bytes = body(
                    request.send().await.map_err(|_| uncertain())?,
                    1024 * 1024,
                    false,
                    &secrets,
                )
                .await?;
                current = serde_json::from_slice(&bytes)
                    .map_err(|_| unavailable("invalid video status response"))?;
            }
            if current.get("usage").is_some() {
                reported_usage = current.get("usage").cloned();
            }
            self.record_usage(usage::generation(reported_usage.as_ref()), None)
                .await?;
            let request = client
                .get(target.url(&format!("videos/{id}/content")))
                .headers(target.headers.clone());
            (
                body(
                    request
                        .send()
                        .await
                        .map_err(|_| unavailable("video download failed"))?,
                    VIDEO_BYTES,
                    false,
                    &secrets,
                )
                .await?,
                Some(id.to_owned()),
            )
        };
        Ok(Generated::File {
            bytes,
            job,
            usage: reported_usage,
        })
    }
}

/// Accept a configured protocol route as the base, matching chat endpoint handling.
fn base(endpoint: &str) -> Result<reqwest::Url, Fault> {
    let mut url = crate::providers::endpoint_url(endpoint)?;
    let path = url.path().trim_end_matches('/');
    let path = ["/responses", "/chat/completions"]
        .into_iter()
        .find_map(|suffix| path.strip_suffix(suffix))
        .unwrap_or(path)
        .to_owned();
    url.set_path(&path);
    Ok(url)
}

async fn body(
    mut response: Response,
    limit: usize,
    initial: bool,
    secrets: &[Secret],
) -> Result<Vec<u8>, Failure> {
    let status = response.status();
    if !status.is_success() {
        let request = response::request_id(&response);
        let bytes = read_body(&mut response, 64 * 1024).await?;
        return Err(Failure::http(
            status.as_u16(),
            &bytes,
            initial,
            request.as_deref(),
            secrets,
        ));
    }
    read_body(&mut response, limit).await.map_err(Into::into)
}

async fn read_body(response: &mut Response, limit: usize) -> Result<Vec<u8>, Fault> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| uncertain())? {
        if chunk.len() > limit - bytes.len() {
            return Err(unavailable("media response exceeds the size limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn unavailable(message: &str) -> Fault {
    Fault::new(ErrorCode::Unavailable, message)
}
fn uncertain() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "media response unavailable; provider outcome may be uncertain",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_configured_protocol_routes() {
        for endpoint in [
            "https://example.test/v1",
            "https://example.test/v1/",
            "https://example.test/v1/responses",
            "https://example.test/v1/chat/completions",
        ] {
            assert_eq!(
                base(endpoint).unwrap().as_str(),
                "https://example.test/v1",
                "{endpoint}"
            );
        }
        assert_eq!(
            base("https://chatgpt.example/backend-api/codex")
                .unwrap()
                .as_str(),
            "https://chatgpt.example/backend-api/codex"
        );
    }

    #[test]
    fn applies_captured_request_policy() {
        use crate::providers::request::fixture;
        use sailry_protocol::{
            Authentication,
            conversation::{
                ModelApi,
                presets::{Binding, RequestPolicy},
            },
        };
        for (authentication, route) in [
            (Authentication::ApiKey, ImageRoute::Responses),
            (Authentication::ChatGptOAuth, ImageRoute::Authorized),
        ] {
            let mut provider = fixture::provider(authentication, ModelApi::Responses);
            provider.binding = Some(Box::new(Binding {
                policies: vec![RequestPolicy {
                    models: vec!["fixture-model".into()],
                    reasoning_replay: None,
                    body: serde_json::from_value(json!({"service_tier":"priority"})).unwrap(),
                }],
                ..Default::default()
            }));
            let prepared = authorization::prepare(
                &provider,
                &fixture::credentials(authentication),
                fixture::metadata("fixture-model", "", &[]),
            )
            .unwrap();
            let target = Target {
                base: base(&prepared.endpoint).unwrap(),
                headers: prepared.headers.clone(),
                prepared,
                provider,
            };
            let (suffix, payload) = target
                .payload(Some(route), "fixture-model", "Draw")
                .unwrap();
            assert_eq!(suffix, "responses");
            assert_eq!(payload["model"], "fixture-model");
            assert_eq!(payload["service_tier"], "priority");
            assert_eq!(payload["stream"], true);
            assert_eq!(payload["store"], false);
            assert_eq!(
                payload["tools"],
                json!([{"type":"image_generation","output_format":"png"}])
            );
            if authentication == Authentication::ChatGptOAuth {
                assert_eq!(payload["prompt_cache_key"], fixture::SESSION);
                assert_eq!(payload["instructions"], "");
            }
            let (_, direct) = target
                .payload(Some(ImageRoute::Images), "fixture-model", "Draw")
                .unwrap();
            assert_eq!(direct, json!({"model":"fixture-model","prompt":"Draw"}));
        }
    }
}
