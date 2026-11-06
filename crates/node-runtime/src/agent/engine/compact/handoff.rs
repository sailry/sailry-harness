//! Hard size bound for an accepted handoff. Lists are ordered most important
//! first, so trimming removes their tails, least essential section first.
use super::{RENDER, tokens};
use goose_context_management::StructuredSummary;

/// Rendered handoff limit; the prompt asks for less.
pub(super) const TOKENS: usize = 4_000;
/// Longer entries are shortened before essential entries are dropped.
const LONG: usize = 2_000;
/// Texts at or below this length are not shortened further.
const SHORTEST: usize = 200;

pub(super) fn bound(summary: &mut StructuredSummary) -> anyhow::Result<()> {
    summary.extra.clear();
    loop {
        let size = tokens::text(&summary.render_with(RENDER)?);
        if size <= TOKENS {
            return Ok(());
        }
        // Remove about the excess, counted per entry, before rendering again.
        let mut excess = size - TOKENS;
        while excess > 0 {
            let Some(removed) = trim(summary) else {
                return Ok(());
            };
            excess = excess.saturating_sub(removed.max(1));
        }
    }
}

/// Makes one reduction and returns its token count, or `None` once nothing
/// more can be removed.
fn trim(summary: &mut StructuredSummary) -> Option<usize> {
    for list in [
        &mut summary.technical_concepts,
        &mut summary.errors_and_fixes,
    ] {
        if let Some(text) = list.pop() {
            return Some(tokens::text(&text));
        }
    }
    if let Some(file) = summary.files.pop() {
        return Some(
            tokens::text(&file.path)
                + tokens::text(&file.summary)
                + file.key_code.as_deref().map_or(0, tokens::text),
        );
    }
    if let Some(removed) = shorten(summary, LONG) {
        return Some(removed);
    }
    for list in [
        &mut summary.problem_solving,
        &mut summary.user_messages,
        &mut summary.pending_tasks,
    ] {
        if let Some(text) = list.pop() {
            return Some(tokens::text(&text));
        }
    }
    if summary.user_intent.len() > 1 {
        return summary.user_intent.pop().map(|text| tokens::text(&text));
    }
    shorten(summary, SHORTEST)
}

/// Halves the longest goal, decision, constraint, task, state or next-step
/// text when it exceeds `minimum` characters.
fn shorten(summary: &mut StructuredSummary, minimum: usize) -> Option<usize> {
    let text = summary
        .user_intent
        .iter_mut()
        .chain(&mut summary.problem_solving)
        .chain(&mut summary.user_messages)
        .chain(&mut summary.pending_tasks)
        .chain(&mut summary.current_work)
        .chain(&mut summary.next_step)
        .max_by_key(|text| text.chars().count())
        .filter(|text| text.chars().count() > minimum)?;
    let half = text.chars().count() / 2;
    let removed: String = text.chars().skip(half).collect();
    let kept: String = text.chars().take(half).collect();
    *text = format!("{}…", kept.trim_end_matches('…'));
    Some(tokens::text(&removed))
}
