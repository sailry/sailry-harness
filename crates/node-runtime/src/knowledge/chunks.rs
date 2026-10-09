//! Source-aligned token windows with bounded overlap and Markdown heading context.
use sailry_protocol::Fault;

pub(super) const TOKENS: usize = 256;
const OVERLAP: usize = 38;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Chunk {
    pub start: usize,
    pub end: usize,
    pub first_line: u32,
    pub last_line: u32,
    pub heading: Option<String>,
}

pub(super) fn split(text: &str, offsets: &[(usize, usize)]) -> Result<Vec<Chunk>, Fault> {
    let mut previous = (0, 0);
    for &(start, end) in offsets {
        if start < previous.0
            || end < previous.1
            || start >= end
            || end > text.len()
            || !text.is_char_boundary(start)
            || !text.is_char_boundary(end)
        {
            return Err(Fault::invalid("invalid knowledge token offsets"));
        }
        previous = (start, end);
    }
    if offsets.is_empty() {
        return Ok(Vec::new());
    }
    let lines = text
        .match_indices('\n')
        .map(|(offset, _)| offset)
        .collect::<Vec<_>>();
    let headings = headings(text);
    let mut chunks = Vec::new();
    let mut token = 0;
    loop {
        let mut next = (token + TOKENS).min(offsets.len());
        if next < offsets.len() {
            let first = offsets[token].0;
            let end = offsets[next - 1].1;
            if let Some(boundary) = text[first..end].rfind("\n\n") {
                let boundary = first + boundary + 2;
                let count = offsets[token..next].partition_point(|(_, end)| *end <= boundary);
                if count >= TOKENS / 2 {
                    next = token + count;
                }
            }
        }
        let start = offsets[token].0;
        let end = offsets[next - 1].1;
        let first_line = u32::try_from(lines.partition_point(|line| *line < start) + 1)
            .map_err(|_| Fault::invalid("knowledge source has too many lines"))?;
        let last_line = u32::try_from(lines.partition_point(|line| *line < end - 1) + 1)
            .map_err(|_| Fault::invalid("knowledge source has too many lines"))?;
        let heading = headings
            .iter()
            .rev()
            .find(|(offset, _)| *offset <= start)
            .map(|(_, text)| text.clone());
        chunks.push(Chunk {
            start,
            end,
            first_line,
            last_line,
            heading,
        });
        if next == offsets.len() {
            break;
        }
        token = next.saturating_sub(OVERLAP).max(token + 1);
    }
    Ok(chunks)
}

fn headings(text: &str) -> Vec<(usize, String)> {
    let mut offset = 0;
    let mut headings = Vec::new();
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        let content = line.trim();
        if content.starts_with("```") || content.starts_with("~~~") {
            fenced = !fenced;
        } else if !fenced {
            let level = content.bytes().take_while(|byte| *byte == b'#').count();
            if (1..=6).contains(&level) && content.as_bytes().get(level) == Some(&b' ') {
                headings.push((
                    offset,
                    content[level + 1..].trim().chars().take(512).collect(),
                ));
            }
        }
        offset += line.len();
    }
    headings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offsets(text: &str) -> Vec<(usize, usize)> {
        text.char_indices()
            .map(|(start, value)| (start, start + value.len_utf8()))
            .collect()
    }

    #[test]
    fn preserves_unicode_windows_and_source_lines() {
        let text = format!(
            "# Heading\n{}\n{}",
            "中文🙂".repeat(100),
            "tail ".repeat(100)
        );
        let offsets = offsets(&text);
        let chunks = split(&text, &offsets).unwrap();
        assert!(chunks.len() > 1);
        assert_eq!(chunks.first().unwrap().start, 0);
        assert_eq!(chunks.last().unwrap().end, text.len());
        for chunk in &chunks {
            assert!(text.is_char_boundary(chunk.start));
            assert!(text.is_char_boundary(chunk.end));
            assert!(chunk.last_line >= chunk.first_line);
            assert_eq!(chunk.heading.as_deref(), Some("Heading"));
            assert!(
                offsets
                    .iter()
                    .filter(|(start, end)| *start >= chunk.start && *end <= chunk.end)
                    .count()
                    <= TOKENS
            );
        }
        assert!(
            chunks
                .windows(2)
                .all(|pair| pair[1].start < pair[0].end && pair[1].end > pair[0].end)
        );
    }

    #[test]
    fn ignores_code_headings_and_prefers_paragraphs() {
        let text = format!(
            "# Real\n```\n# Code\n```\n{}\n\n{}",
            "a".repeat(140),
            "b".repeat(400)
        );
        let chunks = split(&text, &offsets(&text)).unwrap();
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.heading.as_deref() == Some("Real"))
        );
        assert!(text[..chunks[0].end].ends_with("\n\n"));
        assert!(split("🙂", &[(1, 2)]).is_err());
        assert!(split("ab", &[(1, 2), (0, 1)]).is_err());
    }
}
