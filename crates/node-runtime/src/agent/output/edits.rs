//! Edited regions render as numbered text, so the model need not read the file
//! again to verify an edit.
use super::*;
use sailry_protocol::{FileExcerpt, FileWritten};

pub(super) fn project(original: &Value) -> Option<String> {
    let written: FileWritten = serde_json::from_value(original.get("data")?.clone()).ok()?;
    let excerpt = written.excerpt?;
    let mut text = format!(
        "{}\nEdited; {} bytes; revision {}\n",
        written.path, written.size, written.revision
    );
    render(&mut text, &excerpt);
    Some(text)
}

pub(super) fn render(text: &mut String, excerpt: &FileExcerpt) {
    for hunk in &excerpt.hunks {
        let (Some(first), Some(last)) = (hunk.lines.first(), hunk.lines.last()) else {
            continue;
        };
        text.push_str(&format!("Lines {}–{}\n", first.number, last.number));
        for line in &hunk.lines {
            let content = line
                .text
                .strip_suffix('\n')
                .map(|text| text.strip_suffix('\r').unwrap_or(text))
                .unwrap_or(&line.text);
            text.push_str(&format!("{}\t{content}\n", line.number));
        }
        if hunk.shortened {
            text.push_str("[Some lines or text in this region are omitted]\n");
        }
    }
    if excerpt.omitted > 0 {
        text.push_str(&format!(
            "{} more edited regions omitted; read_file shows them\n",
            excerpt.omitted
        ));
    }
}

#[cfg(test)]
mod tests;
