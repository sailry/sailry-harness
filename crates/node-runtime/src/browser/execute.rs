//! Node admission owns authority and artifacts; the initiating controller owns its WebView.
use super::*;
use crate::store::Ingress;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, Output, Request, WorktreeId};
use serde_json::Value;

pub(crate) fn validate(action: &Action) -> Result<(), Fault> {
    if let Action::Navigate { url, .. } | Action::Open { url } = action
        && !url::Url::parse(url).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.username().is_empty()
                && url.password().is_none()
        })
    {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
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
        if !(1..=20000).contains(timeout_ms) || query.is_empty() || query.len() > 2000 {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "invalid browser wait condition or timeout",
            ));
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
    let result = if matches!(action, Action::Screenshot { .. }) {
        save_capture(ingress, *worktree, request.id, result, stop).await?
    } else {
        result
    };
    Ok(Output::Browser(result))
}

async fn save_capture(
    ingress: &Ingress,
    worktree: WorktreeId,
    id: RequestId,
    mut value: Value,
    stop: CancellationToken,
) -> Result<Value, Fault> {
    use base64::Engine;
    let invalid = |message| Fault::new(ErrorCode::InvalidRequest, message);
    let encoded = value
        .as_object_mut()
        .and_then(|value| value.remove("base64_image"))
        .ok_or_else(|| invalid("browser capture is missing image data"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.as_str().unwrap_or_default())
        .map_err(|_| invalid("browser capture contains invalid image data"))?;
    if bytes.len() > 180 * 1024
        || file_format::FileFormat::from_bytes(&bytes)
            != file_format::FileFormat::JointPhotographicExpertsGroup
    {
        return Err(invalid("browser capture must be a bounded JPEG image"));
    }
    let root = ingress.worktree_root(worktree).await?;
    let output = ingress
        .files
        .prepare_media(
            root,
            format!("assets/generated/browser-{id}.jpg"),
            180 * 1024,
        )
        .await?;
    value["path"] = ingress
        .files
        .publish_media(output, bytes, stop)
        .await?
        .into();
    Ok(value)
}
