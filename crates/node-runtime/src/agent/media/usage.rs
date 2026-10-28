//! Media usage joins the same typed session history as ordinary model output.
use super::*;
use crate::agent::canonical;

impl Media {
    pub(super) async fn record_usage(
        &self,
        usage: Option<canonical::Usage>,
        timing: Option<canonical::Timing>,
    ) -> Result<(), Fault> {
        let Some(usage) = usage else {
            return Ok(());
        };
        let event = canonical::Record {
            id: sailry_protocol::RequestId::new().to_string(),
            created: chrono::Utc::now(),
            turn: self.turn,
            provenance: canonical::Provenance {
                author: "system".into(),
                branch: String::new(),
            },
            metadata: canonical::Metadata {
                execution: Some(canonical::Execution {
                    media: Some(self.kind),
                    timing,
                    ..Default::default()
                }),
                runtime: Some(canonical::Runtime {
                    user_visible: false,
                    agent_visible: false,
                    ..Default::default()
                }),
                ..Default::default()
            },
            kind: canonical::Kind::Message(canonical::Message {
                model: Some(self.model.model.clone()),
                usage: Some(usage),
                ..Default::default()
            }),
        };
        self.ingress
            .sessions(self.turn)
            .append_event(&self.session, event)
            .await
    }
}

pub(super) fn generation(value: Option<&Value>) -> Option<canonical::Usage> {
    let value = value?;
    let count = |key: &str| value.get(key).and_then(Value::as_u64);
    if ["input_tokens", "output_tokens", "total_tokens"]
        .iter()
        .any(|key| value.get(key).is_some_and(|value| value.as_u64().is_none()))
    {
        return None;
    }
    let input = count("input_tokens");
    let output = count("output_tokens");
    let total = count("total_tokens");
    if input.is_none() && output.is_none() && total.is_none() {
        return None;
    }
    Some(canonical::Usage {
        input,
        output,
        total,
        provider: Some(value.clone()),
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
            json!({"input_tokens":-1,"output_tokens":2,"total_tokens":1}),
        ] {
            assert!(generation(Some(&value)).is_none());
        }
        let usage = generation(Some(&json!({"input_tokens":1,"output_tokens":2}))).unwrap();
        assert_eq!(usage.input, Some(1));
        assert_eq!(usage.total, None);
        let value = json!({"input_tokens":0,"output_tokens":0,"total_tokens":0});
        let usage = generation(Some(&value)).unwrap();
        assert_eq!(usage.total, Some(0));
        assert_eq!(usage.provider, Some(value));
    }
}
