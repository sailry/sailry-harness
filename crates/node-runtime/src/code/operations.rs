use super::completion::completions;
use super::{diagnostics, documents, server::Server};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, Fault,
    code::{self, Content, Query, Status},
    plugin,
    tool::Operation,
};
use serde_json::{Value, json};

pub(crate) fn handles(operation: Operation) -> bool {
    matches!(
        operation,
        Operation::CodeDiagnostics
            | Operation::CodeDefinition
            | Operation::CodeReferences
            | Operation::CodeHover
            | Operation::CodeSymbols
    )
}

pub(crate) fn description(operation: Operation) -> &'static str {
    match operation {
        Operation::CodeDiagnostics => "Read language-server diagnostics for a saved project file",
        Operation::CodeDefinition => {
            "Locate the definition of a literal symbol on a one-based line"
        }
        Operation::CodeReferences => "Find references to a literal symbol on a one-based line",
        Operation::CodeHover => "Read a symbol's signature and documentation",
        Operation::CodeSymbols => "Read symbols in a file or search the project",
        _ => unreachable!("code operation expected"),
    }
}

pub(crate) fn parameters(operation: Operation) -> Value {
    let path = json!({"type":"string","minLength":1});
    let (properties, required) = match operation {
        Operation::CodeDiagnostics => (
            json!({"path":path,"severity":{"type":["string","null"],"enum":["error","warning","information","hint",null]}}),
            vec!["path"],
        ),
        Operation::CodeDefinition | Operation::CodeReferences | Operation::CodeHover => (
            json!({"path":path,"line":{"type":"integer","minimum":1},"symbol":{"type":"string","minLength":1}}),
            vec!["path", "line", "symbol"],
        ),
        Operation::CodeSymbols => (
            json!({"path":{"type":["string","null"]},"query":{"type":["string","null"]}}),
            vec![],
        ),
        _ => unreachable!("code operation expected"),
    };
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

pub(crate) fn command(
    operation: Operation,
    arguments: Value,
    context: &plugin::Context,
) -> Result<Command, Fault> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        path: Option<String>,
        line: Option<u32>,
        symbol: Option<String>,
        severity: Option<code::Severity>,
        query: Option<String>,
    }
    let input: Input = serde_json::from_value(arguments)
        .map_err(|error| Fault::invalid(format!("invalid code arguments: {error}")))?;
    let worktree = context
        .worktree
        .ok_or_else(|| Fault::invalid("code query requires a worktree"))?;
    let query = if operation == Operation::CodeSymbols {
        if input.line.is_some() || input.symbol.is_some() || input.severity.is_some() {
            return Err(Fault::invalid("invalid symbol search fields"));
        }
        Query::Symbols {
            path: input.path,
            query: input.query,
        }
    } else {
        let path = input
            .path
            .ok_or_else(|| Fault::invalid("code query requires a path"))?;
        if input.query.is_some() {
            return Err(Fault::invalid("query is only valid for symbols"));
        }
        if operation == Operation::CodeDiagnostics {
            if input.line.is_some() || input.symbol.is_some() {
                return Err(Fault::invalid(
                    "diagnostics do not accept a symbol position",
                ));
            }
            Query::Diagnostics {
                path,
                severity: input.severity,
            }
        } else {
            if input.severity.is_some() {
                return Err(Fault::invalid("severity is only valid for diagnostics"));
            }
            let line = input
                .line
                .ok_or_else(|| Fault::invalid("code query requires a line"))?;
            let symbol = input
                .symbol
                .ok_or_else(|| Fault::invalid("code query requires a symbol"))?;
            match operation {
                Operation::CodeDefinition => Query::Definition { path, line, symbol },
                Operation::CodeReferences => Query::References { path, line, symbol },
                Operation::CodeHover => Query::Hover { path, line, symbol },
                _ => unreachable!("code operation expected"),
            }
        }
    };
    Ok(Command::ReadCode {
        worktree,
        query,
        overlay: None,
    })
}

pub(super) async fn read(
    server: &Server,
    query: &Query,
    overlay: Option<&code::Overlay>,
    stop: &CancellationToken,
) -> Result<code::Result, Fault> {
    server.refresh().await?;
    if let Some(path) = query.path() {
        server.synchronize(path, overlay, false).await?;
    } else if overlay.is_some() {
        return Err(Fault::invalid("a code draft requires a document query"));
    }
    let mut partial = false;
    let content = match query {
        Query::Diagnostics { path, severity } => {
            Content::Diagnostics(diagnostics::read(server, path, *severity, stop).await?)
        }
        Query::Definition { path, .. }
        | Query::References { path, .. }
        | Query::Hover { path, .. }
        | Query::DefinitionAt { path, .. }
        | Query::HoverAt { path, .. }
        | Query::Completion { path, .. } => {
            let position = {
                let documents = server.documents.lock().await;
                match query {
                    Query::Definition { line, symbol, .. }
                    | Query::References { line, symbol, .. }
                    | Query::Hover { line, symbol, .. } => {
                        server
                            .encoding
                            .symbol(&documents.0[path].text.text, *line, symbol)?
                    }
                    Query::DefinitionAt { position, .. }
                    | Query::HoverAt { position, .. }
                    | Query::Completion { position, .. } => server
                        .encoding
                        .at(&documents.0[path].text.text, *position)?,
                    _ => unreachable!("position query expected"),
                }
            };
            let method = match query {
                Query::Definition { .. } | Query::DefinitionAt { .. } => "textDocument/definition",
                Query::References { .. } => "textDocument/references",
                Query::Completion { .. } => "textDocument/completion",
                _ => "textDocument/hover",
            };
            let mut params = json!({"textDocument":{"uri":documents::uri(&server.plan.root,path)?},"position":position});
            if matches!(query, Query::References { .. }) {
                params["context"] = json!({"includeDeclaration":true});
            }
            let value = server.request(method, params, stop).await?;
            if matches!(query, Query::Completion { .. }) {
                completions(server, path, value, &mut partial).await?
            } else if matches!(query, Query::Hover { .. } | Query::HoverAt { .. }) {
                let text = hover(value.get("contents").unwrap_or(&Value::Null));
                let (text, truncated) = bounded(&text, 8192);
                partial |= truncated;
                Content::Hover(text)
            } else {
                let values = if value.is_array() {
                    value.as_array().cloned().unwrap_or_default()
                } else if value.is_null() {
                    Vec::new()
                } else {
                    vec![value]
                };
                partial |= values.len() > 100;
                let mut locations = Vec::new();
                for value in values.into_iter().take(100) {
                    if let Some(location) = location(server, &value).await {
                        locations.push(location);
                    }
                }
                Content::Locations(locations)
            }
        }
        Query::Symbols { path, query } => {
            if query
                .as_ref()
                .is_some_and(|query| query.len() > 1024 || query.contains('\0'))
            {
                return Err(Fault::invalid("code symbol query exceeds the limit"));
            }
            let (method, params) = if let Some(path) = path {
                (
                    "textDocument/documentSymbol",
                    json!({"textDocument":{"uri":documents::uri(&server.plan.root,path)?}}),
                )
            } else {
                (
                    "workspace/symbol",
                    json!({"query":query.as_deref().unwrap_or("")}),
                )
            };
            let value = server.request(method, params, stop).await?;
            let mut items = Vec::new();
            let mut queue = value.as_array().cloned().unwrap_or_default();
            let uri = path
                .as_ref()
                .map(|path| documents::uri(&server.plan.root, path))
                .transpose()?;
            while let Some(item) = queue.pop() {
                if items.len() >= 100 {
                    partial = true;
                    break;
                }
                if let Some(children) = item.get("children").and_then(Value::as_array) {
                    queue.extend(children.iter().rev().cloned());
                }
                let value = item.get("location").cloned().or_else(|| Some(json!({"uri":uri.as_ref()?,"range":item.get("selectionRange").or_else(||item.get("range"))?})));
                let location = if let Some(value) = value {
                    location(server, &value).await
                } else {
                    None
                };
                if let (Some(name), Some(kind), Some(location)) = (
                    item.get("name").and_then(Value::as_str),
                    item.get("kind").and_then(Value::as_u64),
                    location,
                ) {
                    items.push(code::Symbol {
                        name: name.into(),
                        kind: kind as u32,
                        location,
                    });
                }
            }
            Content::Symbols(items)
        }
    };
    let state = server.rpc.state.lock().await;
    let status = state.status;
    partial |= status == Status::Indexing;
    drop(state);
    let revision = if let Some(path) = query.path() {
        server
            .documents
            .lock()
            .await
            .0
            .get(path)
            .map(|document| document.text.revision.clone())
    } else {
        None
    };
    let mut result = code::Result {
        status,
        server: Some(server.plan.startup.server.clone()),
        revision,
        content,
        partial,
        notice: (status == Status::Indexing)
            .then(|| "Language indexing is in progress; results may be partial".into()),
        package: None,
        package_info: None,
        startup: None,
        startup_approved: false,
    };
    limit(&mut result)?;
    Ok(result)
}

async fn location(server: &Server, value: &Value) -> Option<code::Location> {
    let uri = value
        .get("uri")
        .or_else(|| value.get("targetUri"))?
        .as_str()?;
    let range = value
        .get("range")
        .or_else(|| value.get("targetSelectionRange"))?;
    let range: lsp_types::Range = serde_json::from_value(range.clone()).ok()?;
    let path = documents::relative(&server.plan.root, uri)?;
    let text = server
        .documents
        .lock()
        .await
        .0
        .get(&path)
        .map(|document| document.text.clone())
        .or_else(|| documents::read(&server.plan.root, &path).ok())?;
    Some(code::Location {
        path,
        start: server.encoding.display(&text.text, range.start)?,
        end: server.encoding.display(&text.text, range.end)?,
    })
}

fn hover(value: &Value) -> String {
    if let Some(value) = value.as_str() {
        value.into()
    } else if let Some(value) = value.get("value").and_then(Value::as_str) {
        value.into()
    } else if let Some(values) = value.as_array() {
        values.iter().map(hover).collect::<Vec<_>>().join("\n\n")
    } else {
        String::new()
    }
}

fn bounded(value: &str, bytes: usize) -> (String, bool) {
    let mut end = value.len().min(bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].into(), end != value.len())
}

pub(crate) fn limit(result: &mut code::Result) -> Result<(), Fault> {
    match &mut result.content {
        Content::Diagnostics(items) => {
            if items.len() > 100 {
                result.partial = true;
                items.truncate(100);
            }
        }
        Content::Locations(items) => {
            if items.len() > 100 {
                result.partial = true;
                items.truncate(100);
            }
        }
        Content::Symbols(items) => {
            if items.len() > 100 {
                result.partial = true;
                items.truncate(100);
            }
        }
        Content::Completions(items) => {
            if items.len() > 100 {
                result.partial = true;
                items.truncate(100);
            }
        }
        Content::Hover(_) => {}
    }
    while serde_json::to_vec(result)
        .map_err(|error| Fault::invalid(error.to_string()))?
        .len()
        > 32 * 1024
    {
        result.partial = true;
        result.notice = Some("Language results reached the output limit".into());
        match &mut result.content {
            Content::Diagnostics(items) if !items.is_empty() => {
                items.pop();
            }
            Content::Locations(items) if !items.is_empty() => {
                items.pop();
            }
            Content::Symbols(items) if !items.is_empty() => {
                items.pop();
            }
            Content::Completions(items) if !items.is_empty() => {
                items.pop();
            }
            Content::Hover(text) if !text.is_empty() => {
                *text = bounded(text, text.len() / 2).0;
            }
            _ if result.package_info.is_some() => {
                result.package_info = None;
            }
            _ => return Err(Fault::invalid("language result exceeds the output limit")),
        }
    }
    Ok(())
}

pub(super) fn empty(query: &Query, package: Option<String>) -> code::Result {
    code::Result {
        status: Status::NotInstalled,
        server: None,
        revision: None,
        content: match query {
            Query::Diagnostics { .. } => Content::Diagnostics(Vec::new()),
            Query::Hover { .. } | Query::HoverAt { .. } => Content::Hover(String::new()),
            Query::Completion { .. } => Content::Completions(Vec::new()),
            Query::Symbols { .. } => Content::Symbols(Vec::new()),
            _ => Content::Locations(Vec::new()),
        },
        partial: false,
        notice: Some(
            if package.is_some() {
                "No candidate language server is installed"
            } else {
                "No installed language service supports this file"
            }
            .into(),
        ),
        package,
        package_info: None,
        startup: None,
        startup_approved: false,
    }
}

#[cfg(test)]
mod tests;
