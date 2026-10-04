//! Public Node operations retain browser ownership without an Agent execution context.
use super::*;
use sailry_protocol::{Command, NodeId, Output, Request, WorktreeId, external_browser::Action};
use serde_json::{Value, json};

pub(crate) struct Call {
    pub(super) ingress: Arc<crate::store::Ingress>,
    pub(super) caller: NodeId,
    pub(super) request: Request,
    pub(super) session: SessionId,
    pub(super) worktree: WorktreeId,
    pub(super) action: Action,
    pub(super) stop: CancellationToken,
}

pub(crate) fn validate(action: Action, arguments: &Value) -> Result<(), Fault> {
    if !arguments.is_object()
        || serde_json::to_vec(arguments).map_or(true, |bytes| bytes.len() > 64 * 1024)
    {
        return Err(invalid(
            "browser arguments must be an object of at most 64 KiB",
        ));
    }
    if action == Action::Wait
        && !arguments
            .get("seconds")
            .and_then(Value::as_f64)
            .is_some_and(|seconds| seconds.is_finite() && (0.0..=30.0).contains(&seconds))
    {
        return Err(invalid("browser wait must be between 0 and 30 seconds"));
    }
    if let Some(url) = arguments.get("url").and_then(Value::as_str)
        && !url::Url::parse(url).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.username().is_empty()
                && url.password().is_none()
        })
    {
        return Err(invalid(
            "external browser URLs must use HTTP or HTTPS without credentials",
        ));
    }
    Ok(())
}

impl Call {
    pub(crate) fn new(
        ingress: Arc<crate::store::Ingress>,
        caller: NodeId,
        request: Request,
        stop: CancellationToken,
    ) -> Self {
        let Command::UseExternalBrowser {
            session,
            worktree,
            action,
            ..
        } = request.command
        else {
            unreachable!("managed browser command expected")
        };
        Self {
            ingress,
            caller,
            request,
            session,
            worktree,
            action,
            stop,
        }
    }

    pub(crate) async fn execute(&self) -> Result<Output, Fault> {
        self.check().await?;
        if self.stop.is_cancelled() || self.ingress.external_browser.stop.is_cancelled() {
            return Err(Fault::new(
                ErrorCode::Cancelled,
                "external browser call cancelled",
            ));
        }
        if self.action == Action::CloseSession {
            self.ingress.external_browser.close(self.session).await?;
            return Ok(Output::ExternalBrowser(
                json!({"closed":true,"execution_node":self.ingress.node}),
            ));
        }
        let mut slot = self
            .ingress
            .external_browser
            .acquire(self.session, &self.stop)
            .await?;
        self.check().await?;
        let session = slot.as_ref().expect("acquired browser session");
        let Command::UseExternalBrowser { arguments, .. } = &self.request.command else {
            unreachable!()
        };
        let mut arguments = arguments.clone();
        let operation = async {
            if matches!(self.action, Action::Downloads | Action::SaveDownload) {
                return self.download(session, self.request.id, &arguments).await;
            }
            let original_upload = arguments.get("file_path").cloned();
            let upload = if self.action == Action::FileUpload {
                Some(self.upload(&session.directory, &mut arguments).await?)
            } else {
                None
            };
            let mut result = operations::execute(&session.browser, self.action, arguments)
                .await
                .map_err(|error| Fault::new(ErrorCode::Unavailable, error.to_string()))?;
            drop(upload);
            if let Some(path) = original_upload {
                result["uploaded_file"] = path;
            }
            self.artifact(self.request.id, &mut result).await?;
            let mut remaining = 48 * 1024;
            let truncated = bound(&mut result, &mut remaining);
            Ok(json!({"execution_node":self.ingress.node,"result":result,"truncated":truncated}))
        };
        let result = tokio::select! {
            biased;
            _ = self.stop.cancelled() => None,
            _ = self.ingress.external_browser.stop.cancelled() => None,
            result = tokio::time::timeout(Duration::from_secs(45), operation) => result.ok(),
        };
        if let Some(result) = result {
            return result.map(Output::ExternalBrowser);
        }
        if let Some(session) = slot.take() {
            session.close().await?;
        }
        Err(Fault::new(
            if self.action.read_only() {
                ErrorCode::Cancelled
            } else {
                ErrorCode::OutcomeUnknown
            },
            if self.action.read_only() {
                "external browser call interrupted; browser session closed"
            } else {
                "external browser call interrupted; effects may have occurred; browser session closed; verify before retrying"
            },
        ))
    }

    async fn check(&self) -> Result<(), Fault> {
        if self.request.plugin.is_some() {
            self.ingress
                .check_plugin(self.caller, self.request.clone())
                .await?;
        }
        Ok(())
    }
}

pub(super) fn invalid(message: impl Into<String>) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}

fn bound(value: &mut Value, remaining: &mut usize) -> bool {
    let mut truncated = false;
    match value {
        Value::String(text) => {
            let mut end = text.len().min(*remaining);
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            truncated = end < text.len();
            text.truncate(end);
            *remaining -= end;
        }
        Value::Array(values) => {
            truncated = values.len() > 128;
            values.truncate(128);
            for value in values {
                truncated |= bound(value, remaining);
            }
        }
        Value::Object(values) => {
            let mut count = 0;
            values.retain(|key, value| {
                count += 1;
                if count > 128 || key.len() > *remaining {
                    truncated = true;
                    return false;
                }
                *remaining -= key.len();
                truncated |= bound(value, remaining);
                true
            });
        }
        _ => {}
    }
    truncated
}
