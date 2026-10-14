//! Reference discovery excludes authored code, links and HTML.
use pulldown_cmark::{Event, Parser, Tag};
use std::ops::Range;

#[cfg(test)]
mod tests;

pub(super) fn excluded(text: &str) -> Vec<Range<usize>> {
    // Code, URLs and authored HTML keep their own syntax rather than becoming
    // composer references. CommonMark supplies byte ranges without rewriting text.
    Parser::new(text)
        .into_offset_iter()
        .filter_map(|(event, range)| {
            matches!(
                event,
                Event::Code(_)
                    | Event::Html(_)
                    | Event::InlineHtml(_)
                    | Event::Start(
                        Tag::CodeBlock(_) | Tag::HtmlBlock | Tag::Link { .. } | Tag::Image { .. }
                    )
            )
            .then_some(range)
        })
        .collect()
}

#[cfg(test)]
pub(super) fn tokens(text: &str) -> Vec<Range<usize>> {
    let excluded = excluded(text);
    let mut result = Vec::new();
    let mut offset = 0;
    for chunk in text.split_inclusive(char::is_whitespace) {
        let token = chunk
            .trim_end()
            .trim_end_matches([',', ';', '!', '?', '，', '。', '；', '！', '？', ')', ']']);
        let mention = token.starts_with('@') && !token[1..].contains('@');
        let command = token.starts_with('/')
            && token[1..]
                .chars()
                .all(|ch| ch.is_alphanumeric() || matches!(ch, ':' | '_' | '-' | '.'));
        let range = offset..offset + token.len();
        if (mention || command)
            && !excluded
                .iter()
                .any(|span| span.start < range.end && span.end > range.start)
        {
            result.push(range);
        }
        offset += chunk.len();
    }
    result
}
