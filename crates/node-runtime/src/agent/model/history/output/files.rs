use super::*;
use sailry_protocol::{FileLines, FileSearch, SearchMode, SearchResults};

const LINE_LIMIT: usize = 2 * 1024;
const MATCH_CHARACTERS: usize = 300;

pub(super) fn lines(original: &Value, source: &Value) -> Option<Value> {
    let page: FileLines = serde_json::from_value(original.get("data")?.clone()).ok()?;
    let mut body = String::new();
    let mut next = page.next_offset;
    let mut last = page.offset;
    let mut omitted = 0;
    for line in &page.lines {
        let text = line.text.trim_end_matches('\n');
        let end = floor(text, LINE_LIMIT);
        let row = if end < text.len() {
            format!(
                "{:>5}\t{} [... {} bytes omitted ...]\n",
                line.number,
                &text[..end],
                text.len() - end
            )
        } else {
            format!("{:>5}\t{text}\n", line.number)
        };
        if body.len() + row.len() > LIMIT - 1024 {
            next = Some(line.number - 1);
            omitted += 1;
            break;
        }
        omitted += usize::from(end < text.len());
        body.push_str(&row);
        last = line.number;
    }
    let mut text = format!(
        "{}\nLines {}–{}; {} bytes; revision {}\n{body}",
        page.path,
        page.offset + usize::from(!body.is_empty()),
        last,
        page.size,
        page.revision
    );
    if omitted > 0 {
        text.push_str("Use read_command with output, offset and limit for omitted text\n");
    }
    if let Some(offset) = next {
        text.push_str(&format!("Continue read_file with offset={offset}\n"));
    }
    Some(json!({"text":text,"output":source,"next_offset":next}))
}

pub(super) fn search(original: &Value, arguments: Option<&Value>, source: &Value) -> Option<Value> {
    let page: SearchResults = serde_json::from_value(original.get("data")?.clone()).ok()?;
    if page.mode != SearchMode::Content {
        return None;
    }
    let options: Option<FileSearch> = arguments
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok());
    let matcher = options.as_ref().and_then(|options| {
        let pattern = if options.regex {
            options.query.clone()
        } else {
            regex::escape(&options.query)
        };
        regex::RegexBuilder::new(&pattern)
            .case_insensitive(!options.case_sensitive)
            .multi_line(true)
            .dot_matches_new_line(options.multiline)
            .size_limit(1 << 20)
            .build()
            .ok()
    });
    let mut grouped = BTreeMap::<&str, Vec<String>>::new();
    for row in &page.matches {
        let found = matcher.as_ref().and_then(|matcher| matcher.find(&row.line));
        let preview = match found {
            Some(found) => snippet(&row.line, found.start()),
            None if !row.matched => snippet(&row.line, 0),
            None => snippet(&row.line, 0),
        };
        grouped.entry(&row.path).or_default().push(format!(
            "{}{} {preview}",
            row.line_number,
            if row.matched { ':' } else { '-' }
        ));
    }
    let text = grouped
        .into_iter()
        .map(|(path, rows)| format!("{path}\n{}", rows.join("\n")))
        .collect::<Vec<_>>()
        .join("\n\n");
    Some(json!({
        "text":text,"output":source,"offset":page.offset,"next_offset":page.next_offset,
        "partial":page.partial,"truncated":page.truncated,"scanned_files":page.scanned_files,
        "skipped":page.skipped,"diagnostics":page.diagnostics
    }))
}

fn snippet(text: &str, found: usize) -> String {
    let positions: Vec<_> = text
        .char_indices()
        .map(|(index, _)| index)
        .chain([text.len()])
        .collect();
    if positions.len() <= MATCH_CHARACTERS + 1 {
        return text.into();
    }
    let matched = positions.partition_point(|index| *index < found);
    let start = matched.saturating_sub(MATCH_CHARACTERS / 3);
    let end = (start + MATCH_CHARACTERS).min(positions.len() - 1);
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        &text[positions[start]..positions[end]],
        if end < positions.len() - 1 { "…" } else { "" }
    )
}
