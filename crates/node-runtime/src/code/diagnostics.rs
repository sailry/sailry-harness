use super::{documents, server::Server};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Fault,
    code::{Diagnostic, Location, Severity},
};
use serde_json::json;

pub(super) async fn read(
    server: &Server,
    path: &str,
    severity: Option<Severity>,
    stop: &CancellationToken,
) -> Result<Vec<Diagnostic>, Fault> {
    let uri = documents::uri(&server.plan.root, path)?;
    if server.capabilities.diagnostic_provider.is_some() {
        let value = server
            .request(
                "textDocument/diagnostic",
                json!({"textDocument":{"uri":uri}}),
                stop,
            )
            .await?;
        if value.get("kind").and_then(serde_json::Value::as_str) == Some("full") {
            let items = serde_json::from_value::<Vec<lsp_types::Diagnostic>>(
                value.get("items").cloned().unwrap_or_else(|| json!([])),
            )
            .map_err(|error| Fault::invalid(format!("invalid language diagnostics: {error}")))?;
            let version = server
                .documents
                .lock()
                .await
                .0
                .get(path)
                .map(|document| document.version);
            server.rpc.state.lock().await.diagnostics.insert(
                uri.clone(),
                super::rpc::Diagnostics {
                    version,
                    items: items
                        .into_iter()
                        .take(101)
                        .map(super::rpc::bounded_diagnostic)
                        .collect(),
                },
            );
        }
    }
    let documents = server.documents.lock().await;
    let Some(document) = documents.0.get(path) else {
        return Ok(Vec::new());
    };
    let state = server.rpc.state.lock().await;
    let Some(published) = state.diagnostics.get(&uri) else {
        return Ok(Vec::new());
    };
    if published
        .version
        .is_some_and(|version| version != document.version)
    {
        return Ok(Vec::new());
    }
    Ok(published
        .items
        .iter()
        .filter_map(|item| {
            let severity_value = match item.severity {
                Some(lsp_types::DiagnosticSeverity::WARNING) => Severity::Warning,
                Some(lsp_types::DiagnosticSeverity::INFORMATION) => Severity::Information,
                Some(lsp_types::DiagnosticSeverity::HINT) => Severity::Hint,
                _ => Severity::Error,
            };
            if severity.is_some_and(|filter| filter != severity_value) {
                return None;
            }
            Some(Diagnostic {
                location: Location {
                    path: path.into(),
                    start: server
                        .encoding
                        .display(&document.text.text, item.range.start)?,
                    end: server
                        .encoding
                        .display(&document.text.text, item.range.end)?,
                },
                severity: severity_value,
                message: item.message.clone(),
                source: item.source.clone(),
                code: item.code.as_ref().map(|code| match code {
                    lsp_types::NumberOrString::Number(value) => value.to_string(),
                    lsp_types::NumberOrString::String(value) => value.clone(),
                }),
            })
        })
        .take(101)
        .collect())
}
