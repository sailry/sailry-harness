//! Independent request policy for compaction summaries.
//! The captured model and account stay the same; the main task configuration is untouched.
use super::*;
use sailry_protocol::conversation::Model as Metadata;

/// Total generation limit, including reasoning for providers that count it.
pub(in crate::agent) const OUTPUT_TOKENS: u32 = 8_000;
/// Transient summary failures are retried at most once. There is no wall-clock
/// deadline: the transport stall timeout and user cancellation still apply.
pub(super) const RETRIES: usize = 1;
pub(super) const RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(5);

/// The summary keeps the captured provider, model and credential and only lowers effort.
pub(super) fn config(provider: &Provider, config: &SessionConfig) -> SessionConfig {
    let effort = match effort(model(provider, config)) {
        Effort::Budget(tokens) => Effort::Budget(tokens.min(max_tokens(provider, config) / 2)),
        effort => effort,
    };
    SessionConfig {
        effort,
        ..config.clone()
    }
}

/// Output limit for the summary request; no additional reasoning allowance.
pub(super) fn max_tokens(provider: &Provider, config: &SessionConfig) -> i32 {
    output_limit(model(provider, config).map_or(0, |model| model.output)) as i32
}

pub(in crate::agent) fn output_limit(output: u32) -> u32 {
    if output == 0 {
        OUTPUT_TOKENS
    } else {
        OUTPUT_TOKENS.min(output)
    }
}

fn model<'a>(provider: &'a Provider, config: &SessionConfig) -> Option<&'a Metadata> {
    provider
        .models
        .iter()
        .find(|model| model.id == config.model)
}

/// The lowest configured reasoning choice. `Default` omits the parameter when
/// the model declares no choices.
pub(super) fn effort(model: Option<&Metadata>) -> Effort {
    model
        .filter(|model| model.reasoning)
        .and_then(|model| {
            model
                .efforts
                .iter()
                .copied()
                .filter_map(|effort| rank(effort).map(|rank| (rank, effort)))
                .min_by_key(|(rank, _)| *rank)
        })
        .map_or(Effort::Default, |(_, effort)| effort)
}

/// Disabled first, then named levels, then explicit token budgets.
fn rank(effort: Effort) -> Option<(u8, i64)> {
    Some(match effort {
        Effort::Disabled => (0, 0),
        Effort::Minimal => (1, 0),
        Effort::Low => (1, 1),
        Effort::Medium => (1, 2),
        Effort::High => (1, 3),
        Effort::XHigh => (1, 4),
        Effort::Max => (1, 5),
        Effort::Budget(tokens) if tokens > 0 => (2, i64::from(tokens)),
        Effort::Budget(_) | Effort::Default => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(efforts: &[Effort], output: u32, reasoning: bool) -> Provider {
        let mut provider: Provider = serde_json::from_value(serde_json::json!({
            "id":sailry_protocol::ProviderId::new(),"revision":0,"name":"Summary fixture",
            "api":"chat_completions","authentication":"api_key","endpoint":"http://127.0.0.1:1",
            "enabled":true,"default_model":"fixture","credential":null,
            "models":[{"id":"fixture","context":200_000,"output":output}]
        }))
        .unwrap();
        provider.models[0].reasoning = reasoning;
        provider.models[0].efforts = efforts.to_vec();
        provider.models[0].default_effort = efforts.last().copied().unwrap_or_default();
        provider
    }

    fn session(effort: Effort) -> SessionConfig {
        SessionConfig {
            assistant: None,
            resource: None,
            provider: sailry_protocol::ProviderId::new(),
            model: "fixture".into(),
            effort,
            features: Default::default(),
            mode: sailry_protocol::WorkMode::Code,
            options: Default::default(),
            permission: sailry_protocol::Permission::Ask,
            credential: None,
        }
    }

    #[test]
    fn selects_lowest_or_default_choice() {
        for (efforts, reasoning, selected, expected) in [
            (
                vec![Effort::High, Effort::Low, Effort::Max],
                true,
                Effort::Max,
                Effort::Low,
            ),
            (
                vec![Effort::Max, Effort::Disabled, Effort::Medium],
                true,
                Effort::Max,
                Effort::Disabled,
            ),
            (
                vec![Effort::Budget(16_000), Effort::Budget(2_048)],
                true,
                Effort::Max,
                Effort::Budget(2_048),
            ),
            (
                vec![Effort::Budget(4_096), Effort::Minimal],
                true,
                Effort::Max,
                Effort::Minimal,
            ),
            (vec![], true, Effort::Max, Effort::Default),
            (vec![Effort::Low], false, Effort::High, Effort::Default),
        ] {
            let provider = provider(&efforts, 128_000, reasoning);
            let main = session(selected);
            let summary = config(&provider, &main);
            assert_eq!(summary.effort, expected, "{efforts:?}");
            assert_eq!(main.effort, selected, "{efforts:?}");
            assert_eq!(summary.model, main.model, "{efforts:?}");
            assert_eq!(summary.provider, main.provider, "{efforts:?}");
        }
    }

    #[test]
    fn bounds_output_independently() {
        let large = provider(&[Effort::Low], 128_000, true);
        assert_eq!(max_tokens(&large, &session(Effort::Low)), 8_000);
        assert_eq!(max_tokens(&large, &session(Effort::Disabled)), 8_000);
        assert_eq!(max_tokens(&large, &session(Effort::Budget(2_048))), 8_000);
        let small = provider(&[Effort::Low], 2_048, true);
        assert_eq!(max_tokens(&small, &session(Effort::Low)), 2_048);
        let plain = provider(&[], 0, false);
        assert_eq!(max_tokens(&plain, &session(Effort::Default)), 8_000);
    }
}
