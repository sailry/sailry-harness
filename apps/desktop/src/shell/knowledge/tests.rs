use super::*;
use core::prelude::v1::test;
use sailry_protocol::{KnowledgeId, knowledge::Citation};

fn citation() -> Citation {
    let mut citation = Citation {
        collection: KnowledgeId::new(),
        chunk: "1".repeat(64),
        revision: "2".repeat(64),
        path: "private/source.xlsx".into(),
        position: Position::Cells {
            sheet: "Sheet".into(),
            start: "A1".into(),
            end: "B4".into(),
        },
        link: String::new(),
    };
    citation.link = citation.url();
    citation
}

#[test]
fn source_identity() {
    for (scenario, check) in [
        (
            "parses_only_machine_source_identity",
            (|_scenario: &str| {
                let citation = citation();
                let parsed = parse(&citation.link).unwrap();
                assert_eq!(
                    parsed.collection, citation.collection,
                    "parses only machine source identity"
                );
                assert_eq!(
                    parsed.chunk, citation.chunk,
                    "parses only machine source identity"
                );
                assert_eq!(
                    parsed.revision, citation.revision,
                    "parses only machine source identity"
                );
                assert_eq!(parsed.offset, 0, "parses only machine source identity");
                for link in [
                    format!("{}&path=../../private", citation.link),
                    format!("{}#L9", citation.link),
                    citation.link.replace("?revision=", "/extra?revision="),
                    citation
                        .link
                        .replace("?revision=", "?revision=bad&revision="),
                    citation.link.replace("sailry-knowledge://", "https://"),
                    citation
                        .link
                        .replace("sailry-knowledge://", "sailry-knowledge://user@"),
                ] {
                    assert!(
                        parse(&link).is_err(),
                        "parses only machine source identity: accepted {link}"
                    );
                }
            }) as fn(&str),
        ),
        (
            "rejects_mismatched_page_identity",
            (|_scenario: &str| {
                let citation = citation();
                let input = parse(&citation.link).unwrap();
                let mut page = CitationPage {
                    citation,
                    title: "Source".into(),
                    text: "text".into(),
                    offset: 0,
                    next_offset: None,
                    worktree: None,
                    context: false,
                    untrusted: true,
                };
                assert!(matches(&page, &input), "rejects mismatched page identity");
                page.context = true;
                assert!(!matches(&page, &input), "rejects mismatched page identity");
                page.context = false;
                page.citation.collection = KnowledgeId::new();
                assert!(!matches(&page, &input), "rejects mismatched page identity");
                page.citation.collection = input.collection;
                page.citation.link = "file:///private/source.xlsx".into();
                assert!(!matches(&page, &input), "rejects mismatched page identity");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
