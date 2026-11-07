//! The language pack locates embedded regions; host attributes refine their grammar.
use super::*;
use std::time::{Duration, Instant};
use tree_sitter::{Node, Query, QueryCursor, StreamingIterator, Tree};

const MAX_REGIONS: usize = 64;

pub(super) fn extract(
    parser: &Parser,
    parsed: &mut Parsed,
    source: &str,
    language: &str,
    tree: Option<&Tree>,
    stop: &CancellationToken,
) -> Result<(), Fault> {
    let Some(query) = get_injections_query(language) else {
        return Ok(());
    };
    let Some(tree) = tree else {
        parsed.partial = true;
        return Ok(());
    };
    let grammar = parser.registry.get_language(language).map_err(|_| {
        Fault::new(
            ErrorCode::NotConfigured,
            "bundled code parser is unavailable",
        )
    })?;
    let query = match Query::new(&grammar, query) {
        Ok(query) => query,
        Err(_) => {
            parsed.partial = true;
            return Ok(());
        }
    };
    let deadline = Instant::now() + Duration::from_millis(PARSE_MILLIS);
    let bytes = source.as_bytes();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), bytes);
    let mut regions = 0usize;
    while let Some(matched) = matches.next() {
        check(stop)?;
        if Instant::now() >= deadline || regions >= MAX_REGIONS {
            parsed.partial = true;
            parsed.truncated = true;
            break;
        }
        let captured_language = matched.captures.iter().find_map(|capture| {
            (query.capture_names()[capture.index as usize] == "injection.language")
                .then(|| capture.node.utf8_text(bytes).ok())
                .flatten()
        });
        let fixed_language = query
            .property_settings(matched.pattern_index)
            .iter()
            .find(|property| property.key.as_ref() == "injection.language")
            .and_then(|property| property.value.as_deref());
        for capture in matched
            .captures
            .iter()
            .filter(|capture| query.capture_names()[capture.index as usize] == "injection.content")
        {
            if regions >= MAX_REGIONS {
                parsed.truncated = true;
                parsed.partial = true;
                break;
            }
            let Some(language) = captured_language.or(fixed_language) else {
                parsed.partial = true;
                continue;
            };
            let language = refinement(capture.node, bytes).unwrap_or_else(|| language.to_owned());
            let Some(text) = source.get(capture.node.byte_range()) else {
                parsed.partial = true;
                continue;
            };
            if parser.registry.get_language(&language).is_err() {
                parsed.partial = true;
                continue;
            }
            match parser.process(text, &language) {
                Ok(result) => {
                    parsed.capabilities.embedded = true;
                    let start = capture.node.start_position();
                    super::extract(
                        parsed,
                        result,
                        &language,
                        text,
                        Origin {
                            byte: capture.node.start_byte(),
                            line: start.row,
                            column: start.column,
                        },
                    );
                }
                Err(_) => parsed.partial = true,
            }
            regions += 1;
        }
    }
    Ok(())
}

fn refinement(node: Node<'_>, source: &[u8]) -> Option<String> {
    let host = node.parent()?;
    if !matches!(host.kind(), "script_element" | "style_element") {
        return None;
    }
    let mut cursor = host.walk();
    let tag = host
        .named_children(&mut cursor)
        .find(|child| child.kind() == "start_tag")?;
    let mut cursor = tag.walk();
    for attribute in tag
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "attribute")
    {
        let text = attribute.utf8_text(source).ok()?;
        let Some((name, value)) = text.split_once('=') else {
            continue;
        };
        if !matches!(name.trim().to_ascii_lowercase().as_str(), "lang" | "type") {
            continue;
        }
        let value = value.trim().trim_matches(['\'', '"']);
        let suffix = value
            .strip_prefix("text/")
            .or_else(|| value.strip_prefix("application/"))
            .unwrap_or(value);
        return Some(
            match suffix {
                "ts" | "typescript" => "typescript",
                "tsx" => "tsx",
                "js" | "javascript" | "module" | "ecmascript" => "javascript",
                "scss" => "scss",
                "sass" => "sass",
                "css" => "css",
                "json" | "ld+json" => "json",
                other => other,
            }
            .to_owned(),
        );
    }
    None
}
