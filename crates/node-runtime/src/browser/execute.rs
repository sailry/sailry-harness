//! Node admission owns authority and artifacts; the initiating controller owns its WebView.
use super::*;
use crate::store::Ingress;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, Output, Request, WorktreeId};
use serde_json::Value;

pub(crate) fn validate(action: &Action) -> Result<(), Fault> {
    if let Action::Navigate { url, .. } | Action::Open { url, .. } = action
        && !url::Url::parse(url).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.username().is_empty()
                && url.password().is_none()
        })
    {
        return Err(Fault::invalid(
            "browser URL must use HTTP or HTTPS without embedded credentials",
        ));
    }
    if let Action::Wait {
        timeout_ms,
        condition,
        ..
    } = action
    {
        let query = match condition {
            browser::Condition::Text { text } => text,
            browser::Condition::Visible { selector } | browser::Condition::Hidden { selector } => {
                selector
            }
        };
        if *timeout_ms == 0 || query.is_empty() {
            return Err(Fault::invalid("invalid browser wait condition or timeout"));
        }
    }
    Ok(())
}

pub(crate) async fn execute(
    ingress: &Ingress,
    caller: NodeId,
    request: &Request,
    stop: CancellationToken,
) -> Result<Output, Fault> {
    let Command::UseBrowser {
        session,
        worktree,
        action,
    } = &request.command
    else {
        unreachable!("controller browser command expected")
    };
    if request.plugin.is_some() {
        ingress.check_plugin(caller, request.clone()).await?;
    }
    if stop.is_cancelled() {
        return Err(Fault::new(ErrorCode::Cancelled, "browser call cancelled"));
    }
    let result = tokio::select! {
        biased;
        _ = stop.cancelled() => Err(Fault::new(
            if action.read_only() { ErrorCode::Cancelled } else { ErrorCode::OutcomeUnknown },
            "browser call interrupted; read the page before trying another action",
        )),
        result = ingress.browsers.call(caller, *session, request.id, action.clone()) => result,
    }?;
    let result = if let Action::Screenshot { format, save, .. } = action {
        let capture = Capture::decode(result, *format)?;
        let path = if *save {
            Some(capture.save(ingress, *worktree, request.id, stop).await?)
        } else {
            None
        };
        capture.publish(path)
    } else {
        result
    };
    Ok(Output::Browser(result))
}

/// The controller supplies pixels and page facts; the Node owns encoding checks and paths.
pub(super) struct Capture {
    facts: serde_json::Map<String, Value>,
    bytes: Vec<u8>,
    format: browser::Format,
}

impl Capture {
    pub(super) fn decode(value: Value, format: browser::Format) -> Result<Self, Fault> {
        use base64::Engine;
        let invalid = |message| Fault::invalid(message);
        let Value::Object(mut facts) = value else {
            return Err(invalid("browser capture is invalid"));
        };
        let encoded = facts
            .remove("base64_image")
            .ok_or_else(|| invalid("browser capture is missing image data"))?;
        for reserved in ["path", "inline_data", "file_data", "images"] {
            facts.remove(reserved);
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.as_str().unwrap_or_default())
            .map_err(|_| invalid("browser capture contains invalid image data"))?;
        let expected = match format {
            browser::Format::Jpeg => file_format::FileFormat::JointPhotographicExpertsGroup,
            browser::Format::Png => file_format::FileFormat::PortableNetworkGraphics,
        };
        let decoded = match format {
            browser::Format::Jpeg => image::ImageFormat::Jpeg,
            browser::Format::Png => image::ImageFormat::Png,
        };
        let dimensions = (bytes.len() <= browser::MAX_IMAGE_BYTES
            && file_format::FileFormat::from_bytes(&bytes) == expected)
            .then(|| {
                image::ImageReader::with_format(std::io::Cursor::new(&bytes), decoded)
                    .into_dimensions()
                    .ok()
            })
            .flatten()
            .filter(|(width, height)| {
                *width > 0 && *height > 0 && (*width).max(*height) <= browser::MAX_IMAGE_SIDE
            })
            .ok_or_else(|| {
                invalid("browser capture must be a bounded image of the requested format")
            })?;
        facts.insert("mime_type".into(), format.mime().into());
        facts.insert("width".into(), dimensions.0.into());
        facts.insert("height".into(), dimensions.1.into());
        Ok(Self {
            facts,
            bytes,
            format,
        })
    }

    async fn save(
        &self,
        ingress: &Ingress,
        worktree: WorktreeId,
        id: RequestId,
        stop: CancellationToken,
    ) -> Result<String, Fault> {
        let root = ingress.worktree_root(worktree).await?;
        let output = ingress
            .files
            .prepare_media(
                root,
                format!("assets/generated/browser-{id}.{}", self.format.extension()),
                browser::MAX_IMAGE_BYTES as u64,
            )
            .await?;
        ingress
            .files
            .publish_media(output, self.bytes.clone(), stop)
            .await
    }

    /// Canonical history stores declared inline media once per digest.
    pub(super) fn publish(self, path: Option<String>) -> Value {
        use base64::Engine;
        let mut value = Value::Object(self.facts);
        if let Some(path) = path {
            value["path"] = path.into();
        }
        value["inline_data"] = serde_json::json!([{
            "mime_type": self.format.mime(),
            "data": base64::engine::general_purpose::STANDARD.encode(&self.bytes),
        }]);
        value
    }
}
