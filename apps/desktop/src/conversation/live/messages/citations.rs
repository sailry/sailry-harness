use sailry_protocol::conversation::Citation;
use std::{borrow::Cow, collections::BTreeMap};

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
    use gpui_kit::{component::text::TextView, *};

    struct Link;

    impl Render for Link {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let sources = [Citation {
                uri: "https://example.com/a_(b)".into(),
                title: None,
                start: Some(0),
                end: Some(0),
            }];
            div().w(px(240.)).child(
                TextView::markdown("source", text("Result", 0, true, &sources).into_owned())
                    .selectable(true),
            )
        }
    }

    #[gpui::test]
    fn opens_numbered_sources(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (_, visual) = cx.add_window_view(|_, _| Link);
        visual.simulate_click(point(px(3.), px(10.)), Modifiers::default());
        assert_eq!(
            visual.opened_url().as_deref(),
            Some("https://example.com/a_(b)")
        );
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
