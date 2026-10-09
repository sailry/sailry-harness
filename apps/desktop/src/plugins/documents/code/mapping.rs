use super::*;

const SCHEME: &str = "sailry-code";

pub(super) fn position(value: code::Position) -> Option<lsp_types::Position> {
    Some(lsp_types::Position::new(
        value.line.checked_sub(1)?,
        value.column.checked_sub(1)?,
    ))
}

pub(super) fn range(value: &code::Location) -> Option<lsp_types::Range> {
    let range = lsp_types::Range::new(position(value.start)?, position(value.end)?);
    (range.start <= range.end).then_some(range)
}

pub(super) fn uri(path: &str) -> Option<lsp_types::Uri> {
    if path.is_empty()
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || path.contains('\\')
    {
        return None;
    }
    let mut url = url::Url::parse("sailry-code://worktree/").ok()?;
    url.path_segments_mut()
        .ok()?
        .clear()
        .extend(path.split('/'));
    url.as_str().parse().ok()
}

pub(super) fn relative_uri(value: &str) -> Option<String> {
    let url = url::Url::parse(value).ok()?;
    if url.scheme() != SCHEME
        || url.host_str() != Some("worktree")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let path = url
        .path_segments()?
        .map(|part| {
            percent_encoding::percent_decode_str(part)
                .decode_utf8()
                .ok()
                .map(|part| part.into_owned())
        })
        .collect::<Option<Vec<_>>>()?
        .join("/");
    (uri(&path)?.as_str() == value).then_some(path)
}

pub(super) fn diagnostics(
    path: &str,
    values: Vec<code::Diagnostic>,
) -> Vec<gpui_kit::component::highlighter::Diagnostic> {
    values
        .into_iter()
        .filter_map(|item| {
            if item.location.path != path {
                return None;
            }
            let severity = match item.severity {
                code::Severity::Error => lsp_types::DiagnosticSeverity::ERROR,
                code::Severity::Warning => lsp_types::DiagnosticSeverity::WARNING,
                code::Severity::Information => lsp_types::DiagnosticSeverity::INFORMATION,
                code::Severity::Hint => lsp_types::DiagnosticSeverity::HINT,
            };
            Some(
                lsp_types::Diagnostic {
                    range: range(&item.location)?,
                    severity: Some(severity),
                    message: item.message,
                    source: item.source,
                    code: item.code.map(lsp_types::NumberOrString::String),
                    ..Default::default()
                }
                .into(),
            )
        })
        .collect()
}
