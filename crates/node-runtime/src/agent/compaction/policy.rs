//! One effective policy for execution and the read-only statistics projection.
use sailry_protocol::conversation::compaction::Budget;

const DEFAULT_TRIGGER: u32 = 262_144;

pub(crate) fn resolve(
    model: &str,
    context: u32,
    output: u32,
    compaction: Option<u32>,
) -> Option<Budget> {
    if context == 0 {
        return None;
    }
    let output_reserve = output.min(context);
    let trigger_tokens = (u64::from(context) * super::CONTEXT_PERCENT / 100)
        .max(1)
        .min(u64::from(compaction.unwrap_or(DEFAULT_TRIGGER).max(1)))
        .min(u64::from(context.saturating_sub(output_reserve)).max(1));
    // Scale both independent windows down with small models. Large models do
    // not need a huge handoff simply because their ordinary output cap is high.
    let summary_output = output
        .max(1)
        .min((context / 8).max(1))
        .min((super::SUMMARY_BYTES / 4) as u32);
    Some(Budget {
        model: model.into(),
        context_limit: context,
        trigger_tokens,
        output_reserve,
        recent_tokens: u64::from(context / 8).min(20_000),
        summary_input: u64::from(context.saturating_sub(summary_output)),
        summary_output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_reserve_and_summary_capacity() {
        let budget = resolve("captured-model", 200_000, 64_000, None).unwrap();
        assert_eq!(budget.model, "captured-model");
        assert_eq!(budget.trigger_tokens, 136_000);
        assert_eq!(budget.output_reserve, 64_000);
        assert_eq!(budget.summary_output, 8192);
        assert_eq!(budget.summary_input, 191_808);
        assert_eq!(budget.recent_tokens, 20_000);
    }

    #[test]
    fn scales_to_small_windows() {
        let budget = resolve("small", 4096, 2048, None).unwrap();
        assert_eq!(budget.trigger_tokens, 2048);
        assert_eq!(budget.output_reserve, 2048);
        assert_eq!(budget.summary_output, 512);
        assert_eq!(budget.summary_input, 3584);
        assert_eq!(budget.recent_tokens, 512);
        assert_eq!(resolve("unknown", 0, 0, None), None);
    }

    #[test]
    fn bounds_large_windows_with_configured_threshold() {
        let budget = resolve("large", 1_050_000, 128_000, None).unwrap();
        assert_eq!(budget.trigger_tokens, 262_144);
        assert_eq!(
            resolve("configured", 1_050_000, 128_000, Some(200_000))
                .unwrap()
                .trigger_tokens,
            200_000
        );
        assert_eq!(
            resolve("small", 128_000, 16_384, Some(300_000))
                .unwrap()
                .trigger_tokens,
            102_400
        );
    }
}
