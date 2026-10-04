//! Generation uses the historical OpenAI-compatible endpoints, without replay.
use super::*;
use base64::Engine as _;
use reqwest::{Client, Response, StatusCode};

const IMAGE_BYTES: usize = 64 * 1024 * 1024;
const VIDEO_BYTES: usize = 512 * 1024 * 1024;

impl Media {
    pub(super) async fn generate(&self, prompt: String, path: String) -> Result<Value, Fault> {
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
        let credential = match &self.model.provider.credential {
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
        let endpoint = format!(
            "{base}/{}",
            if self.kind == Kind::Image {
                "images/generations"
            } else {
                "videos"
            }
        );
        let mut request = client
            .post(endpoint)
            .json(&json!({"model":self.model.model,"prompt":prompt}));
        if let Some(secret) = &credential {
            request = request.bearer_auth(secret.expose());
        }
        let response = request.send().await.map_err(|_| uncertain())?;
        let response_bytes = body(
            response,
            if self.kind == Kind::Image {
                IMAGE_BYTES * 2
            } else {
                1024 * 1024
            },
        )
        .await?;
        let value: Value = serde_json::from_slice(&response_bytes)
            .map_err(|_| unavailable("invalid media generation response"))?;
        let mut reported_usage = value.get("usage").cloned();
        let (bytes, job) = if self.kind == Kind::Image {
            self.record_usage(usage::generation(reported_usage.as_ref()), None)
                .await?;
            let image = value
                .pointer("/data/0")
                .ok_or_else(|| unavailable("generation returned no image"))?;
            let bytes = if let Some(encoded) = image.get("b64_json").and_then(Value::as_str) {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .map_err(|_| unavailable("invalid generated image encoding"))?;
                if bytes.len() > IMAGE_BYTES {
                    return Err(unavailable("generated image exceeds the size limit"));
                }
                bytes
            } else if let Some(url) = image.get("url").and_then(Value::as_str) {
                let url = reqwest::Url::parse(url)
                    .map_err(|_| unavailable("invalid generated image URL"))?;
                if !matches!(url.scheme(), "http" | "https")
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    return Err(unavailable("invalid generated image URL"));
                }
                body(
                    client
                        .get(url)
                        .send()
                        .await
                        .map_err(|_| unavailable("generated image download failed"))?,
                    IMAGE_BYTES,
                )
                .await?
            } else {
                return Err(unavailable("generation returned no image content"));
            };
            if file_format::FileFormat::from_bytes(&bytes)
                != file_format::FileFormat::PortableNetworkGraphics
            {
                return Err(unavailable(
                    "image generation returned an unsupported format; PNG required",
                ));
            }
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
                        return Ok(
                            json!({"job":id,"error":Fault::new(ErrorCode::Unavailable,"video generation failed")}),
                        );
                    }
                    _ => return Err(unavailable("video generation returned an invalid status")),
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
                let mut request = client.get(format!("{base}/videos/{id}"));
                if let Some(secret) = &credential {
                    request = request.bearer_auth(secret.expose());
                }
                let bytes =
                    body(request.send().await.map_err(|_| uncertain())?, 1024 * 1024).await?;
                current = serde_json::from_slice(&bytes)
                    .map_err(|_| unavailable("invalid video status response"))?;
            }
            if current.get("usage").is_some() {
                reported_usage = current.get("usage").cloned();
            }
            self.record_usage(usage::generation(reported_usage.as_ref()), None)
                .await?;
            let mut request = client.get(format!("{base}/videos/{id}/content"));
            if let Some(secret) = &credential {
                request = request.bearer_auth(secret.expose());
            }
            (
                body(
                    request
                        .send()
                        .await
                        .map_err(|_| unavailable("video download failed"))?,
                    VIDEO_BYTES,
                )
                .await?,
                Some(id.to_owned()),
            )
        };
        if self.kind == Kind::Video
            && !matches!(
                file_format::FileFormat::from_bytes(&bytes),
                file_format::FileFormat::Mpeg4Part14 | file_format::FileFormat::Mpeg4Part14Video
            )
        {
            return Err(unavailable(
                "video generation returned an unsupported format; MP4 required",
            ));
        }
        let path = self
            .ingress
            .files
            .publish_media(output, bytes, self.stop.clone())
            .await?;
        Ok(json!({"path":path,"job":job,"usage":reported_usage}))
    }
}

async fn body(mut response: Response, limit: usize) -> Result<Vec<u8>, Fault> {
    let status = response.status();
    if !status.is_success() {
        return Err(Fault::new(
            if matches!(
                status,
                StatusCode::NOT_FOUND
                    | StatusCode::METHOD_NOT_ALLOWED
                    | StatusCode::NOT_IMPLEMENTED
            ) {
                ErrorCode::NotConfigured
            } else {
                ErrorCode::Unavailable
            },
            format!("media endpoint returned HTTP {}", status.as_u16()),
        ));
    }
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
