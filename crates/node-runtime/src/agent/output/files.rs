//! Numbered file text adapted from Sailry fb70fdc1 model/history/output/files.rs.
use super::*;
use sailry_protocol::FileLines;

const LIMIT: usize = 32 * 1024;
const LINE_LIMIT: usize = 2 * 1024;

pub(super) fn project(original: &Value, source: &Value) -> Option<String> {
    let page: FileLines = serde_json::from_value(original.get("data")?.clone()).ok()?;
    let mut body = String::new();
    let mut shortened = false;
    let mut projected = if page.lines.is_empty() {
        Some(render(&page, "", None, false, page.next_offset, source))
    } else {
        None
    };
    for (index, line) in page.lines.iter().enumerate() {
        line.number.checked_sub(1)?;
        let next = match page.lines.get(index + 1) {
            Some(line) => Some(line.number.checked_sub(1)?),
            None => page.next_offset,
        };
        let (row, omitted) = row(line);
        let mut candidate = body.clone();
        candidate.push_str(&row);
        let text = render(
            &page,
            &candidate,
            Some((page.lines[0].number, line.number)),
            shortened || omitted,
            next,
            source,
        );
        // Native providers consume Text directly; plugin adapters may serialize
        // its string. Budget the escaped text too, including metadata and hints.
        if serde_json::to_string(&text).ok()?.len() > LIMIT {
            break;
        }
        body = candidate;
        shortened |= omitted;
        projected = Some(text);
    }
    projected.filter(|text| serde_json::to_string(text).is_ok_and(|encoded| encoded.len() <= LIMIT))
}

fn row(line: &sailry_protocol::FileLine) -> (String, bool) {
    let (text, ending) = if let Some(text) = line.text.strip_suffix("\r\n") {
        (text, "\r\n")
    } else if let Some(text) = line.text.strip_suffix('\n') {
        (text, "\n")
    } else {
        (line.text.as_str(), " [no trailing newline]\n")
    };
    let (end, _) = crate::process::output::edges(text.as_bytes(), LINE_LIMIT, 0);
    let mut row = format!("{}\t{}", line.number, &text[..end]);
    if end < text.len() {
        row.push_str(&format!(
            " [... {} content bytes omitted from this line ...]",
            text.len() - end
        ));
    }
    row.push_str(ending);
    (row, end < text.len())
}

fn render(
    page: &FileLines,
    body: &str,
    range: Option<(usize, usize)>,
    shortened: bool,
    next: Option<usize>,
    source: &Value,
) -> String {
    let mut text = match range {
        Some((first, last)) => format!(
            "{}\nLines {first}–{last}; {} bytes; revision {}\n",
            page.path, page.size, page.revision
        ),
        None => format!(
            "{}\nNo lines at offset={}; {} bytes; revision {}\n",
            page.path, page.offset, page.size, page.revision
        ),
    };
    text.push_str(body);
    if shortened {
        text.push_str(
            "For omitted line text, use read_tool_output with output, offset=0 and limit for the original result JSON\n",
        );
    }
    if let Some(next) = next {
        text.push_str(&format!("Next file page: read_file with offset={next}\n"));
    }
    text.push_str(&json!({"output":source,"next_offset":next}).to_string());
    text
}

#[cfg(test)]
mod tests;
