//! A handoff must contain text and be smaller than its source.
use super::*;

pub(super) fn quality(content: &Content, source_tokens: u64) -> adk_core::Result<()> {
    let text = content.parts.first().and_then(Part::text).unwrap_or("");
    if text.trim().is_empty() {
        return Err(invalid_at(
            Stage::Summary,
            Reason::EmptySummary,
            "context summary is empty",
        ));
    }
    let tokens = 4 + content.parts.iter().map(estimate::part).sum::<u64>();
    if tokens >= source_tokens {
        return Err(invalid_at(
            Stage::Summary,
            Reason::NoReduction,
            "context summary does not reduce the replaced history",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_free_form_handoffs() {
        for text in [
            "Known facts",
            "## Facts\nKnown facts",
            "已确认事实，继续未完成工作",
        ] {
            assert!(quality(&Content::new("user").with_text(text), 1024).is_ok());
        }
    }

    #[test]
    fn requires_smaller_handoffs() {
        let content = Content::new("user").with_text("source ".repeat(160));
        let tokens = 4 + content.parts.iter().map(estimate::part).sum::<u64>();
        assert!(quality(&content, tokens - 1).is_err());
        assert!(quality(&content, tokens).is_err());
        assert!(quality(&content, tokens + 1).is_ok());
        assert!(quality(&content, tokens * 2).is_ok());
    }
}
