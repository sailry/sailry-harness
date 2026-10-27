//! A handoff must be complete, structured, and materially smaller than its source.
use super::*;

pub(super) const HEADINGS: [&str; 8] = [
    "Goals",
    "User constraints",
    "Key facts",
    "Changed files and reasons",
    "Errors and fixes",
    "Todo",
    "Current progress",
    "Next steps",
];

pub(super) fn headings(text: &str) -> bool {
    let mut expected = HEADINGS.iter();
    let mut next = expected.next();
    for line in text.lines() {
        if next.is_some_and(|heading| line.trim_end() == format!("## {heading}")) {
            next = expected.next();
        }
    }
    next.is_none()
}

pub(super) fn quality(content: &Content, source_tokens: u64) -> adk_core::Result<()> {
    let text = content.parts.first().and_then(Part::text).unwrap_or("");
    if text.trim().is_empty() {
        return Err(invalid_at(
            Stage::Summary,
            Reason::EmptySummary,
            "context summary is empty",
        ));
    }
    if !headings(text) {
        return Err(invalid_at(
            Stage::Summary,
            Reason::UnexpectedOutput,
            "context summary is missing required headings",
        ));
    }
    let tokens = 4 + content.parts.iter().map(estimate::part).sum::<u64>();
    if tokens.saturating_mul(2) >= source_tokens {
        return Err(invalid_at(
            Stage::Summary,
            Reason::NoReduction,
            "context summary does not reduce the replaced history by half",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_ordered_level_two_headings() {
        let valid = HEADINGS
            .map(|heading| format!("## {heading}\nKnown fact"))
            .join("\n");
        assert!(headings(&valid));
        assert!(!headings(&valid.replacen("## Goals", "# Goals", 1)));
        assert!(!headings(&valid.replace("## Todo", "## Other")));
        let reversed = HEADINGS
            .into_iter()
            .rev()
            .map(|heading| format!("## {heading}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!headings(&reversed));
    }

    #[test]
    fn rejects_expanding_handoffs() {
        let text = HEADINGS
            .map(|heading| format!("## {heading}\n{}", "source ".repeat(20)))
            .join("\n");
        let content = Content::new("user").with_text(text);
        let tokens = 4 + content.parts.iter().map(estimate::part).sum::<u64>();
        assert!(quality(&content, tokens).is_err());
        assert!(quality(&content, tokens * 2).is_err());
        assert!(quality(&content, tokens * 2 + 1).is_ok());
    }
}
