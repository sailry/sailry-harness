//! The language pack locates embedded regions; host attributes refine their grammar.
use super::*;
use std::{
    ops::ControlFlow,
    time::{Duration, Instant},
};
use tree_sitter::{Node, ParseOptions, Query, QueryCursor, StreamingIterator};
use tree_sitter_language_pack::get_injections_query;

const MAX_REGIONS: usize = 64;

pub(super) fn extract(
    parser: &Parser,
    parsed: &mut Parsed,
    source: &str,
    language: &str,
    stop: &CancellationToken,
) -> Result<(), Fault> {
    let Some(query) = get_injections_query(language) else {
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
    let mut engine = tree_sitter::Parser::new();
    engine.set_language(&grammar).map_err(|_| {
        Fault::new(
            ErrorCode::Internal,
            "embedded parser could not be configured",
        )
    })?;
    let deadline = Instant::now() + Duration::from_millis(PARSE_MILLIS);
    let mut progress = |_: &tree_sitter::ParseState| {
        if stop.is_cancelled() || Instant::now() >= deadline {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let bytes = source.as_bytes();
    let Some(tree) = engine.parse_with_options(
        &mut |offset, _| &bytes[offset..],
        None,
        Some(ParseOptions::new().progress_callback(&mut progress)),
    ) else {
        check(stop)?;
        parsed.partial = true;
        return Ok(());
    };
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
