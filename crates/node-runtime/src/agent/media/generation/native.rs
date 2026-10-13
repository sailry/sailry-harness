use super::*;
use adk_core::{Content, FinishReason, GenerateContentConfig, LlmRequest, LlmResponse, Part};
use sailry_protocol::conversation::ModelApi;

pub(super) async fn generate(
    media: &Media,
    prompt: String,
) -> Result<(Vec<u8>, Option<Value>), Failure> {
    let config = vision::configuration(&media.config, &media.model);
    let output = media
        .model
        .provider
        .models
        .iter()
        .find(|model| model.id == config.model)
        .map_or(8192, |model| model.output);
    let model = model::configured(
        &media.ingress,
        &media.model.provider,
        &config,
        media
            .session
            .parse()
            .map_err(|_| sailry_protocol::Fault::invalid("invalid media session"))?,
        true,
        &media.stop,
    )
    .await?;
    let request = request(media.model.provider.api, &config.model, output, prompt);
    let mut responses =
        model::timing::generate(&model, request, false, &media.identity, media.stop.clone())
            .await
            .map_err(|error| Failure::model(error, true))?;
    let mut image = None;
    let mut completed = false;
    let mut usage = None;
    let mut timing = None;
    let mut received = false;
    while let Some(response) = responses.next().await {
        let response = response.map_err(|error| Failure::model(error, !received))?;
        received = true;
        collect(&mut image, &response)?;
        completed |= !response.partial && response.turn_complete;
        if response.usage_metadata.is_some() {
            usage = response.usage_metadata;
            timing = response.provider_metadata;
        }
    }
    media.record_usage(usage.clone(), timing).await?;
    if !completed {
        return Err(unavailable("image response was not completed").into());
    }
    let image = image.ok_or_else(|| unavailable("generation returned no image"))?;
    Ok((
        image,
        usage.and_then(|value| serde_json::to_value(value).ok()),
    ))
}

fn request(api: ModelApi, model: &str, output: u32, prompt: String) -> LlmRequest {
    let mut extensions = serde_json::Map::new();
    let mut tools = Default::default();
    if api == ModelApi::Responses {
        tools = std::collections::HashMap::from([(
            "image_generation".into(),
            json!({
                "x-adk-openai-tool":{"type":"image_generation","output_format":"png"}
            }),
        )]);
    } else {
        extensions.insert(
            "gemini".into(),
            json!({"response_modalities":["TEXT","IMAGE"]}),
        );
    }
    LlmRequest {
        model: model.into(),
        contents: vec![Content::new("user").with_text(prompt)],
        config: Some(GenerateContentConfig {
            max_output_tokens: Some(output.min(i32::MAX as u32) as i32),
            extensions,
            ..Default::default()
        }),
        tools,
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
            let data = match part {
                Part::InlineData {
                    mime_type, data, ..
                } => {
                    if mime_type != "image/png" {
                        return Err(unavailable(
                            "image generation returned an unsupported format; PNG required",
                        ));
                    }
                    if data.len() > limit {
                        return Err(unavailable("generated image exceeds the size limit"));
                    }
                    data.clone()
                }
                Part::ServerToolCall {
                    server_tool_call: call,
                } if call["type"] == "image_generation_call" => {
                    if call["status"] != "completed" {
                        if response.partial {
                            continue;
                        }
                        return Err(unavailable("image generation was not completed"));
                    }
                    if call
                        .get("output_format")
                        .and_then(Value::as_str)
                        .is_some_and(|format| format != "png")
                    {
                        return Err(unavailable(
                            "image generation returned an unsupported format; PNG required",
                        ));
                    }
                    let encoded = call
                        .get("result")
                        .and_then(Value::as_str)
                        .ok_or_else(|| unavailable("generation returned no image content"))?;
                    super::image::decode_bounded(encoded, limit)?
                }
                _ => continue,
            };
            if image.is_some() {
                return Err(unavailable("generation returned multiple images"));
            }
            *image = Some(data);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_native_image_modalities() {
        let request = request(
            ModelApi::Gemini,
            "captured-model",
            4096,
            "Draw a forest".into(),
        );
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

    #[test]
    fn requests_backend_image_tool() {
        let request = request(
            ModelApi::Responses,
            "captured-main",
            4096,
            "Draw a forest".into(),
        );
        assert_eq!(request.model, "captured-main");
        assert!(request.previous_response_id.is_none());
        assert_eq!(
            request.tools["image_generation"]["x-adk-openai-tool"],
            json!({"type":"image_generation","output_format":"png"})
        );
        assert!(request.config.unwrap().extensions.is_empty());
    }

    #[test]
    fn requires_completed_backend_image() {
        let mut response = LlmResponse {
            content: Some(Content {
                role: "model".into(),
                parts: vec![Part::ServerToolCall {
                    server_tool_call: json!({"type":"image_generation_call","status":"completed","result":"AQID","output_format":"png"}),
                }],
            }),
            turn_complete: true,
            ..Default::default()
        };
        let mut image = None;
        collect_bounded(&mut image, &response, 3).unwrap();
        assert_eq!(image, Some(vec![1, 2, 3]));
        assert!(collect_bounded(&mut None, &response, 2).is_err());
        let Part::ServerToolCall {
            server_tool_call: call,
        } = &mut response.content.as_mut().unwrap().parts[0]
        else {
            unreachable!()
        };
        call["status"] = "in_progress".into();
        assert!(collect(&mut None, &response).is_err());
    }

    #[test]
    fn rejects_incomplete_backend_status() {
        for status in ["incomplete", "failed", "cancelled"] {
            let mut response = response("image/png");
            response.finish_reason = Some(FinishReason::Stop);
            response.provider_metadata = Some(json!({"openai":{"status":status}}));
            let mut image = None;
            assert!(collect(&mut image, &response).is_err());
            assert!(image.is_none());
        }
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
