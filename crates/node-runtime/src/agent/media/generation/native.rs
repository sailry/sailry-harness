use super::*;
use adk_core::{Content, FinishReason, GenerateContentConfig, LlmRequest, LlmResponse, Part};

pub(super) async fn generate(
    media: &Media,
    prompt: String,
) -> Result<(Vec<u8>, Option<Value>), Fault> {
    let config = vision::configuration(&media.config, &media.model)?;
    let metadata = media
        .model
        .provider
        .models
        .iter()
        .find(|model| model.id == config.model)
        .ok_or_else(|| unavailable("frozen image model is unavailable"))?;
    let model = model::configured(
        &media.ingress,
        &media.model.provider,
        &config,
        media
            .session
            .parse()
            .map_err(|_| invalid("invalid media session"))?,
        true,
        &media.stop,
    )
    .await?;
    let request = request(&config.model, metadata.output, prompt);
    let mut responses = model::timing::generate(&model, request, false)
        .await
        .map_err(model::error)?;
    let mut image = None;
    let mut completed = false;
    let mut usage = None;
    let mut timing = None;
    while let Some(response) = responses.next().await {
        let response = response.map_err(model::error)?;
        collect(&mut image, &response)?;
        completed |= !response.partial && response.turn_complete;
        if response.usage_metadata.is_some() {
            usage = response.usage_metadata;
            timing = response.provider_metadata;
        }
    }
    media.record_usage(usage.clone(), timing).await?;
    if !completed {
        return Err(unavailable("image response was not completed"));
    }
    let image = image.ok_or_else(|| unavailable("generation returned no image"))?;
    Ok((
        image,
        usage.and_then(|value| serde_json::to_value(value).ok()),
    ))
}

fn request(model: &str, output: u32, prompt: String) -> LlmRequest {
    let mut extensions = serde_json::Map::new();
    extensions.insert(
        "gemini".into(),
        json!({"response_modalities":["TEXT","IMAGE"]}),
    );
    LlmRequest {
        model: model.into(),
        contents: vec![Content::new("user").with_text(prompt)],
        config: Some(GenerateContentConfig {
            max_output_tokens: Some(output.min(i32::MAX as u32) as i32),
            extensions,
            ..Default::default()
        }),
        tools: Default::default(),
        previous_response_id: None,
    }
}

fn collect(image: &mut Option<Vec<u8>>, response: &LlmResponse) -> Result<(), Fault> {
    collect_bounded(image, response, IMAGE_BYTES)
}

fn collect_bounded(
    image: &mut Option<Vec<u8>>,
    response: &LlmResponse,
    limit: usize,
) -> Result<(), Fault> {
    if response.error_code.is_some() || response.error_message.is_some() || response.interrupted {
        return Err(unavailable("image generation failed"));
    }
    if !response.partial
        && response.turn_complete
        && response
            .finish_reason
            .is_some_and(|reason| reason != FinishReason::Stop)
    {
        return Err(unavailable("image response was not completed"));
    }
    if !response.partial
        && response
            .provider_metadata
            .as_ref()
            .is_some_and(|metadata| metadata["content_complete"].as_bool() == Some(true))
    {
        *image = None;
    }
    if let Some(content) = &response.content {
        for part in &content.parts {
            if let Part::InlineData {
                mime_type, data, ..
            } = part
            {
                if mime_type != "image/png" {
                    return Err(unavailable(
                        "image generation returned an unsupported format; PNG required",
                    ));
                }
                if data.len() > limit {
                    return Err(unavailable("generated image exceeds the size limit"));
                }
                if image.is_some() {
                    return Err(unavailable("generation returned multiple images"));
                }
                *image = Some(data.clone());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_native_image_modalities() {
        let request = request("captured-model", 4096, "Draw a forest".into());
        assert_eq!(request.model, "captured-model");
        assert!(request.previous_response_id.is_none());
        assert!(request.tools.is_empty());
        let config = request.config.unwrap();
        assert_eq!(config.max_output_tokens, Some(4096));
        assert_eq!(
            config.extensions["gemini"]["response_modalities"],
            json!(["TEXT", "IMAGE"])
        );
    }

    fn response(mime_type: &str) -> LlmResponse {
        LlmResponse {
            content: Some(Content {
                role: "model".into(),
                parts: vec![Part::InlineData {
                    mime_type: mime_type.into(),
                    data: vec![1, 2, 3],
                    uri: None,
                    annotations: None,
                }],
            }),
            turn_complete: true,
            ..Default::default()
        }
    }

    #[test]
    fn collects_one_image_part() {
        let mut image = None;
        let response = response("image/png");
        collect(&mut image, &response).unwrap();
        assert_eq!(image, Some(vec![1, 2, 3]));
        assert!(collect(&mut image, &response).is_err());
        let mut image = None;
        assert!(collect_bounded(&mut image, &response, 2).is_err());
        assert!(image.is_none());
    }

    #[test]
    fn rejects_failed_and_unsupported_content() {
        let mut image = None;
        assert!(collect(&mut image, &response("image/jpeg")).is_err());
        let mut response = response("image/png");
        response.error_code = Some("failed".into());
        assert!(collect(&mut image, &response).is_err());
        assert!(image.is_none());
        response.error_code = None;
        response.finish_reason = Some(FinishReason::MaxTokens);
        assert!(collect(&mut image, &response).is_err());
    }

    #[test]
    fn replaces_authoritative_image_content() {
        let mut image = Some(vec![0]);
        let mut response = response("image/png");
        response.provider_metadata = Some(json!({"content_complete":true}));
        collect(&mut image, &response).unwrap();
        assert_eq!(image, Some(vec![1, 2, 3]));
    }
}
