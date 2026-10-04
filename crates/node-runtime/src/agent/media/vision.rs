use super::*;
use adk_core::{Content, GenerateContentConfig, LlmRequest, Part};

impl Media {
    pub(super) async fn inspect(&self, prompt: String, source: Source) -> Result<Value, Fault> {
        let bytes = match source {
            Source::Attachment(id) => {
                let attachment = self.ingress.attachment(self.turn, id).await?;
                if attachment.spec.size > adk_core::MAX_INLINE_DATA_SIZE as u64 {
                    return Err(invalid("image exceeds the model inline limit"));
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
            return Err(invalid("unsupported image input format"));
        }
        let mut config = self.config.clone();
        let provider = &self.model.provider;
        config.provider = provider.id;
        config.model = self.model.model.clone();
        config.credential = provider.credential.clone();
        let metadata = provider
            .models
            .iter()
            .find(|model| model.id == config.model)
            .ok_or_else(|| invalid("frozen vision model is unavailable"))?;
        config.effort = metadata.default_effort;
        let credential = match &config.credential {
            Some(reference) => Some(
                self.ingress
                    .resolve_credential(reference.clone(), provider.id)
                    .await?,
            ),
            None => None,
        };
        let model = model::native(
            provider,
            &config,
            credential.as_ref().map_or("", |secret| secret.expose()),
            config.effort,
            None,
        )?;
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
                max_output_tokens: Some(metadata.output.min(8192) as i32),
                ..Default::default()
            }),
            tools: Default::default(),
            previous_response_id: None,
        };
        let mut responses = model::timing::generate(&model, request, false)
            .await
            .map_err(model::error)?;
        let mut text = String::new();
        let mut usage = None;
        let mut timing = None;
        while let Some(response) = responses.next().await {
            let response = response.map_err(model::error)?;
            if response.error_code.is_some()
                || response.error_message.is_some()
                || response.interrupted
            {
                return Err(Fault::new(ErrorCode::Unavailable, "vision request failed"));
            }
            if let Some(content) = response.content {
                for part in content.parts {
                    if let Part::Text { text: value } = part {
                        text.push_str(&value);
                    }
                }
            }
            if text.len() > 64 * 1024 {
                return Err(invalid("vision response exceeds the size limit"));
            }
            if response.usage_metadata.is_some() {
                usage = response.usage_metadata;
                timing = response.provider_metadata;
            }
        }
        self.record_usage(usage.clone(), timing).await?;
        if text.is_empty() {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                "vision model returned no text",
            ));
        }
        Ok(json!({"text":text,"usage":usage}))
    }
}
