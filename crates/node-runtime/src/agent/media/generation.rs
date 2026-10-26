//! Generation stays on the admitted route, without replay or account fallback.
use super::*;
use crate::providers::media::{ImageRoute, image_api, image_route, supports_api};
use reqwest::{Client, Response};
mod image;
mod native;

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
        let generated = if matches!(route, Some(ImageRoute::Native | ImageRoute::Authorized)) {
            let (bytes, usage) = native::generate(self, prompt).await?;
            Generated::File {
                bytes,
                job: None,
                usage,
            }
        } else {
            self.http(prompt, route).await?
        };
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

    async fn http(&self, prompt: String, route: Option<ImageRoute>) -> Result<Generated, Failure> {
        let config = vision::configuration(&self.config, &self.model);
        let credential = match &config.credential {
            Some(reference) => Some(
                self.ingress
                    .resolve_credential(reference.clone(), self.model.provider.id)
                    .await?,
            ),
            None => None,
        };
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(240))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| unavailable("media HTTP client unavailable"))?;
        let base = self.model.provider.endpoint.trim_end_matches('/');
        let (suffix, payload) = image::request(route, &self.model.model, &prompt);
        let endpoint = format!("{base}/{suffix}");
        let headers = crate::providers::headers(
            self.model.provider.api,
            credential.as_ref(),
            self.model
                .provider
                .binding
                .as_ref()
                .and_then(|binding| binding.key_injection.as_ref()),
        )?;
        let request = client
            .post(endpoint)
            .headers(headers.clone())
            .json(&payload);
        let response = request.send().await.map_err(|_| uncertain())?;
        let response_bytes = body(
            response,
            if self.kind == Kind::Image {
                IMAGE_BYTES * 2
            } else {
                1024 * 1024
            },
            true,
        )
        .await?;
        let value: Value = serde_json::from_slice(&response_bytes)
            .map_err(|_| unavailable("invalid media generation response"))?;
        let mut reported_usage = value.get("usage").cloned();
        let (bytes, job) = if self.kind == Kind::Image {
            self.record_usage(usage::generation(reported_usage.as_ref()), None)
                .await?;
            let bytes = if route == Some(ImageRoute::Responses) {
                image::completed(&value)?
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
                    .get(format!("{base}/videos/{id}"))
                    .headers(headers.clone());
                let bytes = body(
                    request.send().await.map_err(|_| uncertain())?,
                    1024 * 1024,
                    false,
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
                .get(format!("{base}/videos/{id}/content"))
                .headers(headers);
            (
                body(
                    request
                        .send()
                        .await
                        .map_err(|_| unavailable("video download failed"))?,
                    VIDEO_BYTES,
                    false,
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

async fn body(mut response: Response, limit: usize, initial: bool) -> Result<Vec<u8>, Failure> {
    let status = response.status();
    if !status.is_success() {
        let bytes = read_body(&mut response, 64 * 1024).await?;
        return Err(Failure::http(status.as_u16(), &bytes, initial));
    }
    read_body(&mut response, limit).await.map_err(Into::into)
}

async fn read_body(response: &mut Response, limit: usize) -> Result<Vec<u8>, Fault> {
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(unavailable("media response exceeds the size limit"));
    }
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
