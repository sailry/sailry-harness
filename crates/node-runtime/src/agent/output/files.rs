//! Numbered file text adapted from Sailry fb70fdc1 model/history/output/files.rs.
use super::*;
use sailry_protocol::FileLines;
use std::collections::BTreeMap;

const LIMIT: usize = 32 * 1024;
const LINE_LIMIT: usize = 2 * 1024;

/// Complete line ranges already shown to the model, keyed by path and full
/// file revision. A revision is a content hash, so equal revisions mean equal text.
#[derive(Default)]
pub(super) struct Reads(BTreeMap<(String, String), Vec<(usize, usize)>>);

impl Reads {
    fn covers(&self, page: &FileLines, first: usize, last: usize) -> bool {
        self.0
            .get(&(page.path.clone(), page.revision.clone()))
            .is_some_and(|ranges| {
                ranges
                    .iter()
                    .any(|(start, end)| *start <= first && last <= *end)
            })
    }
}

/// `reads` is present only for model-visible results; it stubs a page whose
/// exact lines an earlier visible result already showed in full.
pub(super) fn project(
    original: &Value,
    source: &Value,
    reads: Option<&mut Reads>,
) -> Option<String> {
    let page: FileLines = serde_json::from_value(original.get("data")?.clone()).ok()?;
    let range = page.lines.first().zip(page.lines.last());
    if let (Some(reads), Some((first, last))) = (reads.as_deref(), range)
        && !page.truncated
        && reads.covers(&page, first.number, last.number)
    {
        return Some(unchanged(&page, (first.number, last.number), source));
    }
    let mut body = String::new();
    let mut bytes = 0;
    let mut lines = 0;
    let mut shortened = false;
    let mut range = None;
    let mut next_offset = page.next_offset;
    // Rows stop at the shared line ceiling; the next file page continues there.
    for (index, line) in page.lines.iter().enumerate().take(MAX_LINES) {
        line.number.checked_sub(1)?;
        let next = match page.lines.get(index + 1) {
            Some(line) => Some(line.number.checked_sub(1)?),
            None => page.next_offset,
        };
        let (row, omitted) = row(line, page.truncated);
        let row_bytes = serde_json::to_string(&row).ok()?.len() - 2;
        let row_lines = limit::count(&row);
        let metadata = render(
            &page,
            "",
            Some((page.lines[0].number, line.number)),
            shortened || omitted,
            next,
            source,
        );
        // Native providers consume Text directly; plugin adapters may serialize
        // its string. Budget the escaped text too, including metadata and hints.
        if bytes + row_bytes + serde_json::to_string(&metadata).ok()?.len() > LIMIT
            || lines + row_lines + limit::count(&metadata) > MAX_LINES
        {
            break;
        }
        body.push_str(&row);
        bytes += row_bytes;
        lines += row_lines;
        shortened |= omitted;
        range = Some((page.lines[0].number, line.number));
        next_offset = next;
    }
    let projected = (page.lines.is_empty() || range.is_some())
        .then(|| render(&page, &body, range, shortened, next_offset, source))
        .filter(|text| serde_json::to_string(text).is_ok_and(|encoded| encoded.len() <= LIMIT));
    if !shortened
        && !page.truncated
        && let (Some(reads), Some(range), Some(_)) = (reads, range, &projected)
    {
        reads
            .0
            .entry((page.path.clone(), page.revision.clone()))
            .or_default()
            .push(range);
    }
    projected
}

fn unchanged(page: &FileLines, (first, last): (usize, usize), source: &Value) -> String {
    let mut text = format!(
        "{}
Lines {first}–{last} are unchanged since an earlier read_file result in this conversation; revision {}
",
        page.path, page.revision
    );
    if let Some(next) = page.next_offset {
        text.push_str(&format!("Next file page: read_file with offset={next}\n"));
    }
    text.push_str(&json!({"output":source,"next_offset":page.next_offset}).to_string());
    text
}

fn row(line: &sailry_protocol::FileLine, truncated: bool) -> (String, bool) {
    let (text, ending) = if truncated {
        (line.text.as_str(), " [source line truncated]\n")
    } else if let Some(text) = line.text.strip_suffix("\r\n") {
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
    if page.truncated {
        text.push_str(
            "The source line exceeds the read_file byte limit; its omitted suffix is not in the saved tool output\n",
        );
        if shortened {
            text.push_str(
                "For the saved line prefix, use read_tool_output with output, offset=0 and limit\n",
            );
        }
    } else if shortened {
        text.push_str(
            "For omitted line text, use read_tool_output with output, offset=0 and limit for the original result JSON\n",
        );
    }
    if let Some(next) = next {
        text.push_str(&format!("Next file page: read_file with offset={next}\n"));
    }
    text.push_str(
        &json!({"output":source,"next_offset":next,"truncated":page.truncated}).to_string(),
    );
    text
}

#[cfg(test)]
mod tests;
