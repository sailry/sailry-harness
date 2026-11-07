use super::*;
use base64::Engine as _;
use futures::StreamExt;
use model::{Event, Finish, Message, Usage};
use sailry_protocol::SessionConfig;
use std::time::Instant;

impl Media {
    pub(super) async fn inspect(&self, prompt: String, source: Source) -> Result<Value, Failure> {
        let provider = &self.model.provider;
        let metadata = provider
            .models
            .iter()
            .find(|model| model.id == self.model.model);
        if !crate::providers::media::supports_api(Kind::Vision, provider)
            || metadata.is_some_and(|model| {
                !crate::providers::media::supports(Kind::Vision, provider, model)
            })
        {
            return Err(Failure::unsupported());
        }
        let bytes = match source {
            Source::Attachment(id) => {
                let attachment = self.ingress.attachment(self.turn, id).await?;
                if attachment.spec.size > crate::files::media::INPUT_BYTES as u64 {
                    return Err(Fault::invalid("image exceeds the model inline limit").into());
                }
                self.ingress
                    .attachment_bytes(attachment, self.stop.clone())
                    .await?
            }
            Source::Path(path) => {
                self.ingress
                    .files
                    .media_input(
                        self.ingress.worktree_root(self.worktree).await?,
                        path,
                        self.stop.clone(),
                    )
                    .await?
            }
        };
        let format = file_format::FileFormat::from_bytes(&bytes);
        if !matches!(
            format,
            file_format::FileFormat::PortableNetworkGraphics
                | file_format::FileFormat::JointPhotographicExpertsGroup
                | file_format::FileFormat::Webp
                | file_format::FileFormat::GraphicsInterchangeFormat
        ) {
            return Err(Fault::invalid("unsupported image input format").into());
        }
        let config = configuration(&self.config, &self.model);
        let mut binding = model::binding(
            &self.ingress,
            provider,
            &config,
            self.session
                .parse()
                .map_err(|_| Fault::invalid("invalid media session"))?,
            true,
            &self.stop,
        )
        .await?;
        binding.config.max_tokens = Some(u64::from(
            metadata.map_or(8192, |model| model.output.min(8192)),
        ));
        let messages = vec![Message::user().with_text(prompt).with_image(
            base64::engine::general_purpose::STANDARD.encode(bytes),
            format.media_type(),
        )];
        let started = Instant::now();
        let responses = tokio::select! { biased;
            _ = self.stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "vision request was cancelled").into()),
            result = binding.provider.call(
                binding.config.request(String::new(), messages, vec![]),
                self.stop.clone(),
            ) => result.map_err(|error| Failure::model(error, true))?,
        };
        let (text, mut usage, finish) = tokio::select! { biased;
            _ = self.stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "vision request was cancelled").into()),
            result = collect(responses) => result.map_err(|error| Failure::model(error, false))?,
        };
        usage.elapsed_ms.get_or_insert_with(|| {
            u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
        });
        let native = reported(&usage, finish);
        self.record_usage(
            Some(crate::agent::engine::history::provider_usage(&usage)?),
            Some(crate::agent::canonical::Timing {
                elapsed_us: usage.elapsed_ms.map(|value| value.saturating_mul(1000)),
                first_token_us: usage
                    .time_to_first_token_ms
                    .map(|value| value.saturating_mul(1000)),
            }),
        )
        .await?;
        if text.len() > 64 * 1024 {
            return Err(Fault::invalid("vision response exceeds the size limit").into());
        }
        if text.is_empty() {
            return Err(Fault::new(ErrorCode::Unavailable, "vision model returned no text").into());
        }
        Ok(json!({"text":text,"usage":native}))
    }
}

async fn collect(
    mut stream: model::ModelStream,
) -> Result<(String, Usage, Option<Finish>), model::Error> {
    let mut text = Vec::<String>::new();
    let mut separate = true;
    let mut usage = Usage::default();
    let mut finish = None;
    while let Some(event) = stream.next().await {
        match event? {
            Event::TextDelta { text: delta, .. } => {
                if separate {
                    text.push(delta);
                } else {
                    text.last_mut()
                        .expect("text block was started")
                        .push_str(&delta);
                }
                separate = false;
            }
            Event::ReasoningDelta { .. }
            | Event::RedactedThinking { .. }
            | Event::ToolCall { .. } => separate = true,
            Event::Usage(current) => usage = current,
            Event::Finish(reason) => {
                finish = Some(reason);
                break;
            }
            _ => {}
        }
    }
    Ok((text.join("\n"), usage, finish))
}

fn reported(usage: &Usage, finish: Option<Finish>) -> Value {
    let mut value = json!({
        "model": usage.model.as_deref().unwrap_or_default(),
        "usage": {
            "input_tokens": usage.input,
            "output_tokens": usage.output,
            "total_tokens": usage.total,
            "cache_read_input_tokens": usage.cache_read,
            "cache_write_input_tokens": usage.cache_write,
        },
        "stats": {
            "elapsed_ms": usage.elapsed_ms,
            "time_to_first_token_ms": usage.time_to_first_token_ms,
            "model_load_ms": null,
            "output_tokens": null,
            "draft": null,
        },
    });
    if let Some(cost) = usage.cost {
        value["cost"] = json!(cost);
    }
    if let Some(finish) = finish {
        value["finish_reasons"] = json!([match finish {
            Finish::Stop => "stop".to_owned(),
            Finish::ToolCalls => "tool_calls".into(),
            Finish::Length => "length".into(),
            Finish::ContentFilter => "content_filter".into(),
            Finish::Other(reason) => reason,
        }]);
    }
    let mut additional = usage.provider.as_ref().and_then(Value::as_object).cloned();
    if let Some(reasoning) = usage.reasoning {
        additional
            .get_or_insert_with(Default::default)
            .insert("reasoning_tokens".into(), json!(reasoning));
    }
    if let Some(additional) = additional {
        value["additional_data"] = Value::Object(additional);
    }
    value
}

pub(super) fn configuration(
    primary: &SessionConfig,
    selected: &crate::store::media::Model,
) -> SessionConfig {
    let provider = &selected.provider;
    let metadata = provider
        .models
        .iter()
        .find(|model| model.id == selected.model);
    let mut config = primary.clone();
    if config.provider != provider.id || config.model != selected.model {
        config.features.clear();
    } else if let Some(metadata) = metadata {
        metadata.retain_features(provider.api, &mut config.features);
    } else {
        config.features.clear();
    }
    config.provider = provider.id;
    config.model = selected.model.clone();
    config.credential = provider.credential.clone();
    config.effort = selected
        .effort
        .unwrap_or_else(|| metadata.map_or(Default::default(), |model| model.default_effort));
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::{
        Authentication, CredentialId, CredentialRef, Effort, Permission, ProviderId, WorkMode,
        conversation::{Model, ModelApi, Provider},
    };

    #[tokio::test]
    async fn collects_text_and_latest_usage() {
        let events = vec![
            Ok(Event::TextDelta {
                index: 1,
                text: "Second".into(),
            }),
            Ok(Event::ReasoningDelta {
                index: 2,
                text: Some("Analysis".into()),
                signature: None,
            }),
            Ok(Event::TextDelta {
                index: 0,
                text: "First".into(),
            }),
            Ok(Event::Usage(Usage {
                output: Some(1),
                ..Default::default()
            })),
            Ok(Event::TextDelta {
                index: 0,
                text: " block".into(),
            }),
            Ok(Event::Usage(Usage {
                output: Some(4),
                ..Default::default()
            })),
        ];
        let (text, usage, finish) = collect(Box::pin(futures::stream::iter(events)))
            .await
            .unwrap();
        assert_eq!(text, "Second\nFirst block");
        assert_eq!(usage.output, Some(4));
        assert_eq!(finish, None);
    }

    #[test]
    fn preserves_usage_result_shape() {
        let value = reported(
            &Usage {
                model: Some("vision".into()),
                input: Some(12),
                output: Some(3),
                reasoning: Some(2),
                provider: Some(json!({"fixture":"retained"})),
                elapsed_ms: Some(8),
                ..Default::default()
            },
            Some(Finish::Stop),
        );
        assert_eq!(value["model"], "vision");
        assert_eq!(value["usage"]["input_tokens"], 12);
        assert_eq!(value["usage"]["output_tokens"], 3);
        assert!(value["usage"]["cache_read_input_tokens"].is_null());
        assert_eq!(value["stats"]["elapsed_ms"], 8);
        assert_eq!(value["additional_data"]["reasoning_tokens"], 2);
        assert_eq!(value["additional_data"]["fixture"], "retained");
        assert_eq!(value["finish_reasons"], json!(["stop"]));
        assert!(value.get("cost").is_none());
    }

    fn fixture() -> (SessionConfig, crate::store::media::Model) {
        let provider = Provider {
            request_max_retries: None,
            stream_max_retries: None,
            preset: None,
            binding: None,
            consent: None,
            id: ProviderId::new(),
            revision: 1,
            name: "Vision fixture".into(),
            api: ModelApi::Responses,
            authentication: Authentication::ChatGpt,
            endpoint: "https://example.test/v1".into(),
            options: None,
            oauth: None,
            enabled: true,
            default_model: "main".into(),
            credential: Some(CredentialRef {
                node: NodeId([18; 32]),
                id: CredentialId::new(),
            }),
            models: vec![Model {
                id: "main".into(),
                display_name: None,
                context: 16000,
                output: 4096,
                vision: true,
                tools: true,
                reasoning: true,
                generates: vec![],
                efforts: vec![Effort::High, Effort::Low],
                capabilities: Some(
                    serde_json::from_value(
                        json!({"features":[{"id":"speed","options":[{"id":"fast"}]}]}),
                    )
                    .unwrap(),
                ),
                custom_efforts: false,
                compaction_tokens: None,
                default_effort: Effort::Low,
            }],
        };
        let config = SessionConfig {
            features: std::collections::BTreeMap::from([("speed".into(), "fast".into())]),
            assistant: None,
            resource: None,
            provider: provider.id,
            model: "main".into(),
            effort: Effort::High,
            mode: WorkMode::Code,
            options: Default::default(),
            permission: Permission::Project,
            credential: Some(CredentialRef {
                node: NodeId([18; 32]),
                id: CredentialId::new(),
            }),
        };
        (
            config,
            crate::store::media::Model {
                provider,
                model: "main".into(),
                effort: None,
            },
        )
    }

    #[test]
    fn preserves_the_admitted_configuration() {
        let (primary, mut selected) = fixture();
        selected.provider.credential = primary.credential.clone();
        selected.effort = Some(primary.effort);
        assert_eq!(configuration(&primary, &selected), primary);
    }

    #[test]
    fn preserves_same_model_override_credentials() {
        let (primary, selected) = fixture();
        assert_ne!(primary.credential, selected.provider.credential);
        let config = configuration(&primary, &selected);
        assert_eq!(config.provider, primary.provider);
        assert_eq!(config.model, primary.model);
        assert_eq!(config.credential, selected.provider.credential);
        assert_eq!(config.effort, Effort::Low);
        assert_eq!(config.features, primary.features);
    }

    #[test]
    fn uses_frozen_override_defaults() {
        let (primary, mut selected) = fixture();
        selected.provider.id = ProviderId::new();
        let config = configuration(&primary, &selected);
        assert_eq!(config.provider, selected.provider.id);
        assert_eq!(config.credential, selected.provider.credential);
        assert_eq!(config.effort, Effort::Low);
        assert_eq!(config.permission, primary.permission);
        assert!(config.features.is_empty());
        selected.model = "unavailable".into();
        let config = configuration(&primary, &selected);
        assert_eq!(config.model, selected.model);
        assert_eq!(config.effort, Effort::Default);
        assert_eq!(config.credential, selected.provider.credential);
    }
}
