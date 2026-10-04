use adk_core::{LlmResponse, Part};
use sailry_protocol::conversation::Citation;

pub(super) fn project(response: &LlmResponse) -> Vec<Citation> {
    let length: usize = response
        .content
        .as_ref()
        .into_iter()
        .flat_map(|content| &content.parts)
        .map(|part| match part {
            Part::Text { text } => text.chars().count(),
            _ => 0,
        })
        .sum();
    response
        .citation_metadata
        .as_ref()
        .into_iter()
        .flat_map(|metadata| &metadata.citation_sources)
        .filter_map(|source| {
            let uri = reqwest::Url::parse(source.uri.as_deref()?).ok()?;
            if !matches!(uri.scheme(), "http" | "https") || !uri.has_host() {
                return None;
            }
            let range = source
                .start_index
                .zip(source.end_index)
                .filter(|(start, end)| *start >= 0 && end >= start && *end as usize <= length);
            Some(Citation {
                uri: uri.into(),
                title: source.title.clone(),
                start: range.map(|(start, _)| start as u32),
                end: range.map(|(_, end)| end as u32),
            })
        })
        .chain(
            response
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata["groundingChunks"].as_array())
                .into_iter()
                .flatten()
                .filter_map(|chunk| {
                    let source = &chunk["web"];
                    let uri = reqwest::Url::parse(source["uri"].as_str()?).ok()?;
                    if !matches!(uri.scheme(), "http" | "https") || !uri.has_host() {
                        return None;
                    }
                    Some(Citation {
                        uri: uri.into(),
                        title: source["title"].as_str().map(str::to_owned),
                        // Grounded answers are displayed unchanged with the provider's HTML.
                        start: None,
                        end: None,
                    })
                }),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use adk_core::{CitationMetadata, CitationSource, Content};

    #[test]
    fn preserves_sources_and_checks_unicode_ranges() {
        let source = CitationSource {
            uri: Some("https://example.com/source".into()),
            title: Some("Source".into()),
            start_index: Some(0),
            end_index: Some(3),
            license: None,
            publication_date: None,
        };
        let response = LlmResponse {
            content: Some(Content::new("model").with_text("结果🙂")),
            citation_metadata: Some(CitationMetadata {
                citation_sources: vec![
                    source.clone(),
                    CitationSource {
                        end_index: Some(4),
                        ..source.clone()
                    },
                    CitationSource {
                        start_index: Some(-1),
                        ..source.clone()
                    },
                    CitationSource {
                        uri: Some("file:///etc/passwd".into()),
                        ..source.clone()
                    },
                    CitationSource {
                        uri: None,
                        ..source
                    },
                ],
            }),
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
