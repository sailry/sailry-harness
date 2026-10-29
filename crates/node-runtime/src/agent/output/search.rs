//! Compact search previews affect only the outgoing model copy.
use super::*;
use sailry_protocol::{SearchMode, SearchResults};

const LIMIT: usize = 32 * 1024;
const LINE_LIMIT: usize = 2 * 1024;

pub(super) fn project(original: &Value, source: &Value) -> Option<String> {
    let page: SearchResults = serde_json::from_value(original.get("data")?.clone()).ok()?;
    let total = match page.mode {
        SearchMode::Content => page.matches.len(),
        SearchMode::Files => page.paths.len(),
        SearchMode::Count => page.counts.len(),
    };
    let mut body = String::new();
    let mut path = None;
    let mut shown = 0;
    let mut shortened = false;
    for index in 0..total {
        let mut candidate = body.clone();
        let mut omitted = false;
        match page.mode {
            SearchMode::Content => {
                let row = &page.matches[index];
                if path != Some(row.path.as_str()) {
                    candidate.push_str(&label(&row.path));
                    candidate.push('\n');
                }
                let (line, cut) = preview(&row.line);
                candidate.push_str(&format!(
                    "{}{}\t{line}",
                    row.line_number,
                    if row.matched { ':' } else { '-' }
                ));
                if cut {
                    candidate.push_str(" [preview shortened]");
                }
                if row.truncated {
                    candidate.push_str(" [source line truncated]");
                }
                candidate.push('\n');
                omitted = cut || row.truncated;
            }
            SearchMode::Files => {
                candidate.push_str(&label(&page.paths[index]));
                candidate.push('\n');
            }
            SearchMode::Count => {
                let row = &page.counts[index];
                candidate.push_str(&format!("{}\t{}\n", row.count, label(&row.path)));
            }
        }
        let text = render(
            &page,
            &candidate,
            index + 1,
            shortened || omitted,
            source,
            "",
        );
        if serde_json::to_string(&text).ok()?.len() > LIMIT {
            break;
        }
        body = candidate;
        shown = index + 1;
        shortened |= omitted;
        if page.mode == SearchMode::Content {
            path = Some(page.matches[index].path.as_str());
        }
    }
    let mut diagnostics = String::new();
    for diagnostic in &page.diagnostics {
        let (message, cut) = preview(&diagnostic.message);
        let row = format!(
            "Diagnostic {}: {message}{}\n",
            label(&diagnostic.path),
            if cut { " [preview shortened]" } else { "" }
        );
        let candidate = format!("{diagnostics}{row}");
        let text = render(&page, &body, shown, shortened || cut, source, &candidate);
        if serde_json::to_string(&text).ok()?.len() > LIMIT {
            shortened = true;
            break;
        }
        diagnostics = candidate;
        shortened |= cut;
    }
    let text = render(&page, &body, shown, shortened, source, &diagnostics);
    (serde_json::to_string(&text).ok()?.len() <= LIMIT).then_some(text)
}

fn label(path: &str) -> String {
    if path.chars().any(char::is_control) {
        serde_json::to_string(path).expect("search path is a JSON string")
    } else {
        path.into()
    }
}

fn preview(text: &str) -> (&str, bool) {
    let (end, _) = crate::process::output::edges(text.as_bytes(), LINE_LIMIT, 0);
    (&text[..end], end < text.len())
}

fn render(
    page: &SearchResults,
    body: &str,
    shown: usize,
    shortened: bool,
    source: &Value,
    diagnostics: &str,
) -> String {
    let (mode, total) = match page.mode {
        SearchMode::Content => ("content", page.matches.len()),
        SearchMode::Files => ("files", page.paths.len()),
        SearchMode::Count => ("count", page.counts.len()),
    };
    let next = if page.partial || shown == 0 {
        None
    } else if shown < total {
        page.offset.checked_add(shown)
    } else {
        page.next_offset
    };
    let mut text = format!(
        "Search {mode}; offset={}; {shown} rows; scanned {}; skipped {}\n",
        page.offset, page.scanned_files, page.skipped,
    );
    if page.mode == SearchMode::Content {
        text.push_str("':' marks a match; '-' marks context\n");
    }
    text.push_str(body);
    text.push_str(diagnostics);
    if shortened || shown < total || !page.diagnostics.is_empty() {
        text.push_str("Use read_tool_output with output, offset=0 and limit for the original result JSON; read_file for source text beyond a truncated search line\n");
    }
    if page.partial {
        text.push_str("Incomplete scan: narrow path before retrying; no continuation\n");
    } else if let Some(next) = next {
        text.push_str(&format!(
            "Next search page: repeat search_files with the same options and offset={next}\n"
        ));
    }
    text.push_str(
        &json!({"output":source,"next_offset":next,"partial":page.partial,
            "truncated":page.truncated || shortened || shown < total})
        .to_string(),
    );
    text
}

#[cfg(test)]
mod tests;
