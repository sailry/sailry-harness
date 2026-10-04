//! Editable Markdown uses Kit's syntax theme and the original Textarea engine.
#[cfg(test)]
use gpui_kit::{
    App, Entity, HighlightStyle,
    base::input::{TextDecoration, TextareaLink, TextareaState},
    component::ActiveTheme,
};
use pulldown_cmark::{Event, Parser, Tag};
use std::ops::Range;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) fn apply(input: &Entity<TextareaState>, cx: &mut App) {
    let text = input.read(cx).value();
    let spans = styles(&text, cx);
    input.update(cx, |input, cx| input.set_decorations(spans, cx));
}

#[cfg(test)]
pub(super) fn apply_links(input: &Entity<TextareaState>, links: Vec<TextareaLink>, cx: &mut App) {
    let text = input.read(cx).value();
    let spans = styles(&text, cx);
    let color = cx.theme().link;
    let style = HighlightStyle {
        color: Some(color),
        ..Default::default()
    };
    let spans: Vec<TextDecoration> = gpui_kit::combine_highlights(
        spans.into_iter().map(|span| (span.range, span.style)),
        links.iter().map(|link| (link.range.clone(), style)),
    )
    .map(|(range, style)| TextDecoration::new(range, style))
    .collect();
    let masks = links
        .iter()
        .filter(|link| link.icon.is_some())
        .filter_map(|link| {
            Some((
                link.marker_range(&text)?,
                HighlightStyle {
                    // Highlight colors blend; fading hides the source marker instead.
                    fade_out: Some(1.),
                    ..Default::default()
                },
            ))
        });
    let spans = gpui_kit::combine_highlights(
        spans
            .into_iter()
            .map(|span: TextDecoration| (span.range, span.style)),
        masks,
    )
    .map(|(range, style)| TextDecoration::new(range, style))
    .collect();
    input.update(cx, |input, cx| {
        input.set_decorations(spans, cx);
        input.set_links(links, cx);
    });
}

#[cfg(test)]
fn styles(text: &str, cx: &App) -> Vec<TextDecoration> {
    let syntax = crate::content::syntax::highlight("markdown", text, cx);
    let color = cx.theme().link;
    let token_style = HighlightStyle {
        color: Some(color),
        ..Default::default()
    };
    gpui_kit::combine_highlights(
        syntax,
        tokens(text).into_iter().map(|range| (range, token_style)),
    )
    .map(|(range, style)| TextDecoration::new(range, style))
    .collect()
}

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
