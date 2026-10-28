use crate::agent::canonical::{BlockKind, Message};
use sailry_protocol::conversation::Citation;

pub(super) fn project(response: &Message) -> Vec<Citation> {
    let length: usize = response
        .content
        .iter()
        .map(|block| match &block.kind {
            BlockKind::Text { text } => text.chars().count(),
            _ => 0,
        })
        .sum();
    response
        .sources
        .citations
        .iter()
        .filter_map(|source| {
            let uri = reqwest::Url::parse(source.uri.as_deref()?).ok()?;
            if !matches!(uri.scheme(), "http" | "https") || !uri.has_host() {
                return None;
            }
            let range = source
                .start
                .zip(source.end)
                .filter(|(start, end)| *start >= 0 && end >= start && *end as usize <= length);
            Some(Citation {
                uri: uri.into(),
                title: source.title.clone(),
                start: range.map(|(start, _)| start as u32),
                end: range.map(|(_, end)| end as u32),
            })
        })
        .chain(response.sources.grounding.iter().filter_map(|source| {
            let uri = reqwest::Url::parse(&source.uri).ok()?;
            if !matches!(uri.scheme(), "http" | "https") || !uri.has_host() {
                return None;
            }
            Some(Citation {
                uri: uri.into(),
                title: source.title.clone(),
                // Grounded answers are displayed unchanged with the provider's HTML.
                start: None,
                end: None,
            })
        }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::canonical::{Block, Citation as Source, Sources};

    #[test]
    fn preserves_sources_and_checks_unicode_ranges() {
        let source = Source {
            uri: Some("https://example.com/source".into()),
            title: Some("Source".into()),
            start: Some(0),
            end: Some(3),
        };
        let response = Message {
            part_count: 1,
            content: vec![Block {
                index: 0,
                kind: BlockKind::Text {
                    text: "结果🙂".into(),
                },
            }],
            sources: Sources {
                citations: vec![
                    source.clone(),
                    Source {
                        end: Some(4),
                        ..source.clone()
                    },
                    Source {
                        start: Some(-1),
                        ..source.clone()
                    },
                    Source {
                        uri: Some("file:///etc/passwd".into()),
                        ..source.clone()
                    },
                    Source {
                        uri: None,
                        ..source
                    },
                ],
                ..Default::default()
            },
            ..Default::default()
        };
        let citations = project(&response);
        assert_eq!(citations.len(), 3);
        assert_eq!(citations[0].start, Some(0));
        assert_eq!(citations[0].end, Some(3));
        assert_eq!(citations[0].title.as_deref(), Some("Source"));
        assert_eq!(citations[1].end, None);
        assert_eq!(citations[2].start, None);
    }
}
