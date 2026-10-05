//! Recover paired literal strong markers in prose without rewriting stored source.
use pulldown_cmark::{CowStr, Event, Parser, Tag, TagEnd};
use std::ops::Range;

pub(super) fn parse<'a>(source: &'a str, mut emit: impl FnMut(Event<'a>, Range<usize>)) {
    let mut pending = Vec::new();
    let mut code = false;
    for (event, range) in Parser::new_ext(source, super::OPTIONS).into_offset_iter() {
        if matches!(event, Event::Start(Tag::CodeBlock(_))) {
            code = true;
        }
        if code {
            flush(source, &mut pending, &mut emit);
            if matches!(event, Event::End(TagEnd::CodeBlock)) {
                code = false;
            }
            emit(event, range);
            continue;
        }
        if let Event::Text(text) = event {
            pending.push((text, range));
        } else {
            flush(source, &mut pending, &mut emit);
            emit(event, range);
        }
    }
    flush(source, &mut pending, &mut emit);
}

fn flush<'a>(
    source: &str,
    pending: &mut Vec<(CowStr<'a>, Range<usize>)>,
    emit: &mut impl FnMut(Event<'a>, Range<usize>),
) {
    if pending.is_empty() {
        return;
    }
    let mut text = String::new();
    let mut eligible = Vec::new();
    for (value, range) in pending.iter() {
        let start = text.len();
        // Escaped markers and decoded entities must remain literal.
        if source.get(range.clone()) == Some(value.as_ref()) {
            eligible.extend(
                value
                    .match_indices('*')
                    .filter(|(index, _)| {
                        source[..range.start + index]
                            .bytes()
                            .rev()
                            .take_while(|byte| *byte == b'\\')
                            .count()
                            % 2
                            == 0
                    })
                    .map(|(index, _)| start + index),
            );
        }
        text.push_str(value);
    }
    let markers: Vec<_> = text
        .match_indices("**")
        .map(|(at, _)| at)
        .filter(|at| {
            eligible.binary_search(at).is_ok()
                && eligible.binary_search(&(at + 1)).is_ok()
                && (at == &0 || text.as_bytes()[at - 1] != b'*')
                && text.as_bytes().get(at + 2) != Some(&b'*')
        })
        .collect();
    if markers.len() < 2 {
        for (text, range) in pending.drain(..) {
            emit(Event::Text(text), range);
        }
        return;
    }
    let range = pending.first().unwrap().1.start..pending.last().unwrap().1.end;
    let mut at = 0;
    for pair in markers.as_chunks::<2>().0 {
        let (open, close) = (pair[0], pair[1]);
        let content = &text[open + 2..close];
        let trimmed = content.trim_end_matches([' ', '\t']);
        if trimmed.is_empty() || trimmed.starts_with(char::is_whitespace) || content.contains('\n')
        {
            continue;
        }
        // CommonMark leaves CJK punctuation-adjacent delimiters and closing
        // delimiters preceded by a space literal. Only these paired prose runs
        // are recovered; code, HTML, nested marks and links flush this buffer.
        emit(Event::Text(text[at..open].to_owned().into()), range.clone());
        emit(Event::Start(Tag::Strong), range.clone());
        emit(Event::Text(trimmed.to_owned().into()), range.clone());
        emit(Event::End(TagEnd::Strong), range.clone());
        emit(
            Event::Text(content[trimmed.len()..].to_owned().into()),
            range.clone(),
        );
        at = close + 2;
    }
    emit(Event::Text(text[at..].to_owned().into()), range);
    pending.clear();
}

#[cfg(test)]
mod tests {
    use crate::content::markdown::{
        doc::{BlockKind, Mark, Part},
        parse::with_ranges,
    };

    #[test]
    fn recovers_prose_and_preserves_source_ranges() {
        for source in [
            "**未测试项： **界面颜色",
            "**未测试项：**界面颜色",
            "before **label: **after",
        ] {
            let parsed = with_ranges(source);
            let text = parsed.doc.blocks[0]
                .text_at(Part::Body)
                .unwrap_or_else(|| panic!("paragraph expected: {source}"));
            assert!(!text.text.contains("**"));
            assert!(text.marks.iter().any(|span| span.mark == Mark::Bold));
            assert_eq!(&source[parsed.block_ranges[0].clone()], source);
        }
    }

    #[test]
    fn keeps_code_escapes_and_unfinished_markers_literal() {
        for source in [
            r"\*\*label: \*\*",
            "`**label: **`",
            "**unfinished",
            "before ****",
            "before ** ** after",
        ] {
            let parsed = with_ranges(source);
            let text = parsed.doc.blocks[0]
                .text_at(Part::Body)
                .unwrap_or_else(|| panic!("paragraph expected: {source}"));
            assert!(
                text.marks.iter().all(|span| span.mark != Mark::Bold),
                "{source}"
            );
            assert!(text.text.contains('*'));
        }
        for source in ["****", "** **"] {
            let parsed = with_ranges(source);
            assert!(matches!(parsed.doc.blocks[0].kind, BlockKind::Rule));
        }
        let parsed = with_ranges("```text\n**label: **\n```\n");
        let BlockKind::Code { code, .. } = &parsed.doc.blocks[0].kind else {
            panic!("code block expected")
        };
        assert!(code.text.contains("**label: **"));
    }
}
