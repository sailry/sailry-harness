//! Hard size bound for an accepted handoff. Lists are ordered most important
//! first, so trimming removes their tails, least essential section first.
use super::{structured::Summary, tokens};

/// Rendered handoff limit; the prompt asks for less.
pub(super) const TOKENS: usize = 4_000;
/// Tokens one goal, constraint, task, state or next-step text may keep before
/// any of those entries is dropped.
const SHARE: usize = TOKENS / 8;
/// Texts at or below this many tokens are not shortened further.
const SHORTEST: usize = 48;

pub(super) fn bound(summary: &mut Summary) -> anyhow::Result<()> {
    summary.extra.clear();
    loop {
        let size = tokens::text(&summary.render()?);
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
/// more can be removed. References and decisions go first, then long texts
/// are shortened, then goals, constraints and tasks beyond the first of each.
fn trim(summary: &mut Summary) -> Option<usize> {
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
    if let Some(text) = summary.problem_solving.pop() {
        return Some(tokens::text(&text));
    }
    if let Some(removed) = shorten(summary, SHARE) {
        return Some(removed);
    }
    for list in [
        &mut summary.pending_tasks,
        &mut summary.user_messages,
        &mut summary.user_intent,
    ] {
        if list.len() > 1 {
            return list.pop().map(|text| tokens::text(&text));
        }
    }
    shorten(summary, SHORTEST)
}

/// Halves the longest essential text, never below `minimum` tokens, and
/// returns the tokens removed.
fn shorten(summary: &mut Summary, minimum: usize) -> Option<usize> {
    let (size, text) = summary
        .user_intent
        .iter_mut()
        .chain(&mut summary.user_messages)
        .chain(&mut summary.pending_tasks)
        .chain(&mut summary.current_work)
        .chain(&mut summary.next_step)
        .map(|text| (tokens::text(text), text))
        .max_by_key(|(size, _)| *size)
        .filter(|(size, _)| *size > minimum)?;
    let target = (size / 2).max(minimum);
    let characters = text.chars().count();
    let kept: String = text
        .chars()
        .take((characters * target / size).max(1))
        .collect();
    *text = format!("{}…", kept.trim_end_matches('…'));
    Some(size.saturating_sub(tokens::text(text)))
}
