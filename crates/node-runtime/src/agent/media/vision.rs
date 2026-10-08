use super::*;
use adk_core::{Content, GenerateContentConfig, LlmRequest, LlmResponse, Part};
use sailry_protocol::SessionConfig;

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
                if attachment.spec.size > adk_core::MAX_INLINE_DATA_SIZE as u64 {
                    return Err(invalid("image exceeds the model inline limit").into());
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
            return Err(invalid("unsupported image input format").into());
        }
        let config = configuration(&self.config, &self.model);
        let model = model::configured(
            &self.ingress,
            provider,
            &config,
            self.session
                .parse()
                .map_err(|_| invalid("invalid media session"))?,
            true,
            &self.stop,
        )
        .await?;
        let mut content = Content::new("user").with_text(prompt);
        content.parts.push(Part::InlineData {
            mime_type: format.media_type().into(),
            data: bytes,
            annotations: None,
            uri: None,
        });
        let request = LlmRequest {
            model: config.model.clone(),
            contents: vec![content],
            config: Some(GenerateContentConfig {
                max_output_tokens: Some(
                    metadata.map_or(8192, |model| model.output.min(8192)) as i32
                ),
                ..Default::default()
            }),
            tools: Default::default(),
            previous_response_id: None,
        };
        let mut responses = model::timing::generate(&model, request, false)
            .await
            .map_err(|error| Failure::model(error, true))?;
        let mut text = String::new();
        let mut usage = None;
        let mut timing = None;
        let mut completed = false;
        let mut received = false;
        while let Some(response) = responses.next().await {
            let response = response.map_err(|error| Failure::model(error, !received))?;
            received = true;
            append(&mut text, &response)?;
            completed |= !response.partial && response.turn_complete;
            if response.usage_metadata.is_some() {
                usage = response.usage_metadata;
                timing = response.provider_metadata;
            }
        }
        self.record_usage(usage.clone(), timing).await?;
        if !completed {
            return Err(
                Fault::new(ErrorCode::Unavailable, "vision response was not completed").into(),
            );
        }
        if text.is_empty() {
            return Err(Fault::new(ErrorCode::Unavailable, "vision model returned no text").into());
        }
        Ok(json!({"text":text,"usage":usage}))
    }
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
    config.provider = provider.id;
    config.model = selected.model.clone();
    config.credential = provider.credential.clone();
    config.effort = selected
        .effort
        .unwrap_or_else(|| metadata.map_or(Default::default(), |model| model.default_effort));
    config
}

fn append(text: &mut String, response: &LlmResponse) -> Result<(), Fault> {
    if response.error_code.is_some()
        || response.error_message.is_some()
        || response.interrupted
        || response.provider_metadata.as_ref().is_some_and(|metadata| {
            matches!(
                metadata["openai"]["status"].as_str(),
                Some("incomplete" | "failed" | "cancelled")
            )
        })
    {
        return Err(Fault::new(ErrorCode::Unavailable, "vision request failed"));
    }
    if !response.partial
        && response
            .provider_metadata
            .as_ref()
            .is_some_and(|metadata| metadata["content_complete"].as_bool() == Some(true))
    {
        text.clear();
    }
    if let Some(content) = &response.content {
        for part in &content.parts {
            if let Part::Text { text: value } = part {
                if value.len() > 64 * 1024 - text.len() {
                    return Err(invalid("vision response exceeds the size limit"));
                }
                text.push_str(value);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::{
        Authentication, CredentialId, CredentialRef, Effort, Permission, ProviderId, WorkMode,
        conversation::{Model, ModelApi, Provider},
    };

    fn fixture() -> (SessionConfig, crate::store::media::Model) {
        let provider = Provider {
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
                web_search: false,
                generates: vec![],
                efforts: vec![Effort::High, Effort::Low],
                capabilities: None,
                custom_efforts: false,
                default_effort: Effort::Low,
            }],
        };
        let config = SessionConfig {
            assistant: None,
            resource: None,
            provider: provider.id,
            model: "main".into(),
            effort: Effort::High,
            mode: WorkMode::Code,
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
        selected.model = "unavailable".into();
        let config = configuration(&primary, &selected);
        assert_eq!(config.model, selected.model);
        assert_eq!(config.effort, Effort::Default);
        assert_eq!(config.credential, selected.provider.credential);
    }

    #[test]
    fn replaces_authoritative_stream_content() {
        let mut text = String::new();
        let mut response = LlmResponse {
            content: Some(Content::new("model").with_text("A")),
            partial: true,
            ..Default::default()
        };
        append(&mut text, &response).unwrap();
        response.partial = false;
        response.content = Some(Content::new("model").with_text("Answer"));
        response.provider_metadata = Some(json!({"content_complete":true}));
        append(&mut text, &response).unwrap();
        assert_eq!(text, "Answer");
        response.provider_metadata = Some(json!({"openai":{"status":"failed"}}));
        assert!(append(&mut text, &response).is_err());
        assert_eq!(text, "Answer");
    }
}
