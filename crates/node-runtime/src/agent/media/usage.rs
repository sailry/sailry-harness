use super::*;
use adk_core::{Event, UsageMetadata};
use adk_session::SessionService;

impl Media {
    pub(super) async fn record_usage(
        &self,
        usage: Option<UsageMetadata>,
        timing: Option<Value>,
    ) -> Result<(), Fault> {
        let Some(usage) = usage else {
            return Ok(());
        };
        // Reuse canonical ADK history; ownership comes from the frozen session binding.
        let mut event = Event::new(self.turn.to_string());
        event.author = "system".into();
        event.provider_metadata.insert(
            "sailry_media".into(),
            match self.kind {
                Kind::Vision => "vision",
                Kind::Image => "image",
                Kind::Video => "video",
            }
            .into(),
        );
        event.llm_response.usage_metadata = Some(usage);
        event.llm_response.provider_metadata = timing;
        self.ingress
            .sessions(self.turn)
            .append_event(&self.session, event)
            .await
            .map_err(model::error)
    }
}

// The compatible image API reports aggregate token counts plus modality details.
// Missing or invalid counters stay unknown; retain the native breakdown for history.
pub(super) fn generation(value: Option<&Value>) -> Option<UsageMetadata> {
    let value = value?;
    let count = |key: &str| i32::try_from(value.get(key)?.as_u64()?).ok();
    Some(UsageMetadata {
        prompt_token_count: count("input_tokens")?,
        candidates_token_count: count("output_tokens")?,
        total_token_count: count("total_tokens")?,
        provider_usage: Some(value.clone()),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_unknown_usage() {
        for value in [
            Value::Null,
            json!({"seconds":8}),
            json!({"input_tokens":1,"output_tokens":2}),
            json!({"input_tokens":-1,"output_tokens":2,"total_tokens":1}),
            json!({"input_tokens":1,"output_tokens":2147483648u64,"total_tokens":2147483649u64}),
        ] {
            assert!(generation(Some(&value)).is_none());
        }
        let value = json!({"input_tokens":0,"output_tokens":0,"total_tokens":0});
        let usage = generation(Some(&value)).unwrap();
        assert_eq!(usage.total_token_count, 0);
        assert_eq!(usage.provider_usage, Some(value));
    }
}
