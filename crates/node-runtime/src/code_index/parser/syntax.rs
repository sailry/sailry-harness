//! The language pack keeps its extraction tree private. One bounded parse
//! serves both embedded regions and references.
use super::*;
use std::{
    ops::ControlFlow,
    time::{Duration, Instant},
};
use tree_sitter::{ParseOptions, Tree};

/// Returns no tree when cancellation or the parse budget stops parsing.
pub(super) fn parse(
    parser: &Parser,
    language: &str,
    source: &str,
    stop: &CancellationToken,
) -> Result<Option<Tree>, Fault> {
    let grammar = parser.registry.get_language(language).map_err(|_| {
        Fault::new(
            ErrorCode::NotConfigured,
            "bundled code parser is unavailable",
        )
    })?;
    let mut engine = tree_sitter::Parser::new();
    engine
        .set_language(&grammar)
        .map_err(|_| Fault::new(ErrorCode::Internal, "syntax parser could not be configured"))?;
    let deadline = Instant::now() + Duration::from_millis(PARSE_MILLIS);
    let mut progress = |_: &tree_sitter::ParseState| {
        if stop.is_cancelled() || Instant::now() >= deadline {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let bytes = source.as_bytes();
    let tree = engine.parse_with_options(
        &mut |offset, _| &bytes[offset..],
        None,
        Some(ParseOptions::new().progress_callback(&mut progress)),
    );
    if tree.is_none() {
        check(stop)?;
    }
    Ok(tree)
}
