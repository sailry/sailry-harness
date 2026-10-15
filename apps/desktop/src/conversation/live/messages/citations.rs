use sailry_protocol::conversation::Citation;
use std::{borrow::Cow, collections::BTreeMap};

/// Native provider reference IDs have no URL. Hide only their complete known
/// display artifact, leaving canonical text and copy/export ownership unchanged.
pub(super) fn presentation(source: &str) -> Cow<'_, str> {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    const OPEN: &str = "\u{e200}cite\u{e202}";
    const CLOSE: char = '\u{e201}';
    if !source.contains(OPEN) {
        return Cow::Borrowed(source);
    }
    let parser = Parser::new_ext(source, Options::ENABLE_MATH);
    let mut protected: Vec<_> = parser
        .reference_definitions()
        .iter()
        .map(|(_, definition)| definition.span.clone())
        .collect();
    for (event, range) in parser.into_offset_iter() {
        if matches!(
            event,
            Event::Start(Tag::CodeBlock(_) | Tag::HtmlBlock | Tag::Link { .. } | Tag::Image { .. })
                | Event::Code(_)
                | Event::Html(_)
                | Event::InlineHtml(_)
                | Event::InlineMath(_)
                | Event::DisplayMath(_)
        ) {
            protected.push(range);
        }
    }
    let mut output = String::new();
    let mut copied = 0;
    for (start, _) in source.match_indices(OPEN) {
        if start < copied {
            continue;
        }
        let body = start + OPEN.len();
        let Some(length) = source[body..].find(CLOSE) else {
            continue;
        };
        let end = body + length + CLOSE.len_utf8();
        if source[body..body + length].split('\u{e202}').all(reference)
            && !protected
                .iter()
                .any(|range| start < range.end && range.start < end)
        {
            output.push_str(&source[copied..start]);
            copied = end;
        }
    }
    if copied == 0 {
        Cow::Borrowed(source)
    } else {
        output.push_str(&source[copied..]);
        Cow::Owned(output)
    }
}

fn reference(value: &str) -> bool {
    let Some(value) = value.strip_prefix("turn") else {
        return false;
    };
    let digits = value.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return false;
    }
    let value = &value[digits..];
    let Some(value) = value
        .strip_prefix("search")
        .or_else(|| value.strip_prefix("fetch"))
    else {
        return false;
    };
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Citation offsets belong to the complete entry; Markdown belongs to this view.
pub(super) fn text<'a>(
    text: &'a str,
    offset: usize,
    last: bool,
    citations: &[Citation],
) -> Cow<'a, str> {
    if citations.is_empty() {
        return Cow::Borrowed(text);
    }
    let boundaries: Vec<_> = text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()))
        .collect();
    let length = boundaries.len() - 1;
    let mut insertions: BTreeMap<usize, Vec<(usize, &Citation)>> = BTreeMap::new();
    for (index, citation) in citations.iter().enumerate() {
        let position = match citation.end {
            Some(end)
                if (end as usize > offset || end == 0 && offset == 0)
                    && end as usize <= offset + length =>
            {
                Some(boundaries[end as usize - offset])
            }
            None if last => Some(text.len()),
            _ => None,
        };
        if let Some(position) = position {
            insertions
                .entry(position)
                .or_default()
                .push((index + 1, citation));
        }
    }
    if insertions.is_empty() {
        return Cow::Borrowed(text);
    }
    let mut output = String::with_capacity(text.len());
    let mut copied = 0;
    for (position, sources) in insertions {
        output.push_str(&text[copied..position]);
        for (number, citation) in sources {
            // Angle-delimited Markdown destinations preserve parentheses in URLs.
            let uri = citation
                .uri
                .replace('<', "%3C")
                .replace('>', "%3E")
                .replace('\\', "%5C")
                .replace('\n', "%0A")
                .replace('\r', "%0D");
            output.push_str(&format!(" [{number}](<{uri}>)"));
        }
        copied = position;
    }
    output.push_str(&text[copied..]);
    Cow::Owned(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn hides_complete_native_reference_artifacts() {
        let marker = "\u{e200}cite\u{e202}turn0search2\u{e202}turn1fetch3\u{e201}";
        let source = format!("Result{marker} [source](https://example.com) tail");
        assert_eq!(
            presentation(&source),
            "Result [source](https://example.com) tail"
        );
        assert!(source.contains(marker), "canonical input is unchanged");
        for end in 0..marker.len() {
            if marker.is_char_boundary(end) {
                assert_eq!(presentation(&marker[..end]), &marker[..end]);
            }
        }
        let sources = [Citation {
            uri: "https://example.com".into(),
            title: None,
            start: None,
            end: None,
        }];
        let resolved = text(&source, 0, true, &sources);
        assert!(presentation(&resolved).contains("[1](<https://example.com>)"));
    }

    #[test]
    fn preserves_literal_and_unknown_artifacts() {
        let marker = "\u{e200}cite\u{e202}turn0search2\u{e201}";
        for source in [
            format!("`{marker}`"),
            format!("```text\n{marker}\n```"),
            format!("[{marker}](https://example.com)"),
            format!("[source](https://example.com/{marker})"),
            format!("<div>\n{marker}\n</div>"),
            format!("<span data-citation=\"{marker}\"></span>"),
            format!("${marker}$"),
            "\u{e200}cite\u{e202}unknown\u{e201}".into(),
            "\u{e200}cite\u{e202}turn0search\u{e201}".into(),
            "\u{e200}cite\u{e202}turn0search2junk\u{e201}".into(),
            "\u{e200}other\u{e202}turn0search2\u{e201}".into(),
            "Private literal \u{e000} stays".into(),
        ] {
            assert_eq!(presentation(&source), source);
        }
    }

    #[test]
    fn places_sources_across_parts() {
        let source = Citation {
            uri: "https://example.com/a_(b)".into(),
            title: Some("Source".into()),
            start: Some(0),
            end: Some(3),
        };
        let sources = vec![
            source.clone(),
            Citation {
                start: None,
                end: None,
                ..source
            },
        ];
        assert_eq!(
            text("结果🙂", 0, false, &sources),
            "结果🙂 [1](<https://example.com/a_(b)>)"
        );
        assert_eq!(
            text("More", 3, true, &sources),
            "More [2](<https://example.com/a_(b)>)"
        );
    }

    #[test]
    fn preserves_unrelated_parts() {
        let sources = [Citation {
            uri: "https://example.com".into(),
            title: None,
            start: Some(3),
            end: Some(5),
        }];
        assert!(matches!(
            text("🙂", 0, false, &sources),
            Cow::Borrowed("🙂")
        ));
        assert_eq!(
            text("ab", 3, true, &sources),
            "ab [1](<https://example.com>)"
        );
    }
}
