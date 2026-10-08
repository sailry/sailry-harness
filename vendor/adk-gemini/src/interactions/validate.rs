//! Client-side validation for interaction requests.
//!
//! All constraint checks run in [`validate_interaction_request`] before the
//! request is dispatched to the backend. This catches configuration errors
//! early with clear, actionable messages rather than relying on opaque server
//! rejections.

use crate::client::Error;

use super::model::{Content, CreateInteractionRequest, Input, Tool};

/// Known Antigravity agent IDs.
const ANTIGRAVITY_AGENTS: &[&str] = &["antigravity-preview-05-2026"];

/// Known Deep Research agent IDs.
const DEEP_RESEARCH_AGENTS: &[&str] = &[
    "deep-research-preview-04-2026",
    "deep-research-max-preview-04-2026",
    "deep-research-pro-preview-12-2025",
];

/// Validate an interaction request against managed-agent constraints.
///
/// This function enforces:
/// - Model/agent mutual exclusivity
/// - Antigravity constraints (no background, no unsupported gen params, no
///   response_format, no function tools, no audio/video/document input)
/// - Deep Research constraints (background must be true)
///
/// Returns `Ok(())` if the request passes all checks, or an `Error::Validation`
/// with a descriptive message identifying the specific violation.
pub(crate) fn validate_interaction_request(
    request: &CreateInteractionRequest,
) -> Result<(), Error> {
    // 1. Model/agent mutual exclusivity
    if request.model.is_some() && request.agent.is_some() {
        return Err(Error::Validation {
            message: "`model` and `agent` are mutually exclusive; set one or the other".to_string(),
        });
    }

    let agent = request.agent.as_deref();

    // 2. Antigravity constraints
    if agent.is_some_and(|a| ANTIGRAVITY_AGENTS.contains(&a)) {
        validate_antigravity(request)?;
    }

    // 3. Deep Research constraints
    if agent.is_some_and(|a| DEEP_RESEARCH_AGENTS.contains(&a)) {
        validate_deep_research(request)?;
    }

    Ok(())
}

/// Validate Antigravity-specific constraints.
fn validate_antigravity(request: &CreateInteractionRequest) -> Result<(), Error> {
    // 2a. background must not be true
    if request.background == Some(true) {
        return Err(Error::Validation {
            message: "Antigravity does not support background execution; \
                      remove `background(true)` from the builder"
                .to_string(),
        });
    }

    // 2b. No unsupported generation parameters
    if let Some(ref gc) = request.generation_config {
        let mut unsupported = Vec::new();
        if gc.temperature.is_some() {
            unsupported.push("temperature");
        }
        if gc.top_p.is_some() {
            unsupported.push("top_p");
        }
        if gc.max_output_tokens.is_some() {
            unsupported.push("max_output_tokens");
        }
        if !gc.stop_sequences.is_empty() {
            unsupported.push("stop_sequences");
        }
        if !unsupported.is_empty() {
            return Err(Error::Validation {
                message: format!(
                    "Antigravity does not support generation parameters: {}",
                    unsupported.join(", ")
                ),
            });
        }
    }

    // 2c. No structured-output response format
    if request.response_format.is_some() {
        return Err(Error::Validation {
            message: "Antigravity does not support structured-output response format".to_string(),
        });
    }

    // 2d. No custom function tools
    let has_function_tools = request.tools.iter().any(|t| matches!(t, Tool::Function { .. }));
    if has_function_tools {
        return Err(Error::Validation {
            message: "Antigravity does not support custom function-calling tools; \
                      supported tools: code_execution, google_search, url_context"
                .to_string(),
        });
    }

    // 2e. No unsupported input modalities
    if let Input::Content(ref contents) = request.input {
        for content in contents {
            match content {
                Content::Audio(_) => {
                    return Err(Error::Validation {
                        message: "Antigravity does not support audio input; \
                                  only text and image inputs are supported"
                            .to_string(),
                    });
                }
                Content::Video(_) => {
                    return Err(Error::Validation {
                        message: "Antigravity does not support video input; \
                                  only text and image inputs are supported"
                            .to_string(),
                    });
                }
                Content::Document(_) => {
                    return Err(Error::Validation {
                        message: "Antigravity does not support document input; \
                                  only text and image inputs are supported"
                            .to_string(),
                    });
                }
                _ => {}
            }
        }
    }

    Ok(())
}

/// Validate Deep Research-specific constraints.
fn validate_deep_research(request: &CreateInteractionRequest) -> Result<(), Error> {
    if request.background != Some(true) {
        return Err(Error::Validation {
            message: "Deep Research requires background execution; \
                      set `background(true)` on the builder"
                .to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interactions::model::{GenerationConfig, Input, ResponseFormat};

    fn base_request() -> CreateInteractionRequest {
        CreateInteractionRequest { input: Input::Text("test".to_string()), ..Default::default() }
    }

    #[test]
    fn test_model_agent_mutual_exclusivity() {
        let mut req = base_request();
        req.model = Some("models/gemini-2.5-flash".to_string());
        req.agent = Some("antigravity-preview-05-2026".to_string());

        let err = validate_interaction_request(&req).unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn test_antigravity_rejects_background() {
        let mut req = base_request();
        req.agent = Some("antigravity-preview-05-2026".to_string());
        req.background = Some(true);

        let err = validate_interaction_request(&req).unwrap_err();
        assert!(err.to_string().contains("background execution"));
    }

    #[test]
    fn test_antigravity_rejects_unsupported_gen_params() {
        let mut req = base_request();
        req.agent = Some("antigravity-preview-05-2026".to_string());
        req.generation_config =
            Some(GenerationConfig { temperature: Some(0.7), ..Default::default() });

        let err = validate_interaction_request(&req).unwrap_err();
        assert!(err.to_string().contains("temperature"));
    }

    #[test]
    fn test_antigravity_rejects_response_format() {
        let mut req = base_request();
        req.agent = Some("antigravity-preview-05-2026".to_string());
        req.response_format = Some(ResponseFormat::json_schema(serde_json::json!({})));

        let err = validate_interaction_request(&req).unwrap_err();
        assert!(err.to_string().contains("response format"));
    }

    #[test]
    fn test_antigravity_rejects_function_tools() {
        let mut req = base_request();
        req.agent = Some("antigravity-preview-05-2026".to_string());
        req.tools = vec![Tool::function("my_fn", "desc", serde_json::json!({}))];

        let err = validate_interaction_request(&req).unwrap_err();
        assert!(err.to_string().contains("function-calling tools"));
    }

    #[test]
    fn test_antigravity_rejects_audio_input() {
        let mut req = base_request();
        req.agent = Some("antigravity-preview-05-2026".to_string());
        req.input = Input::Content(vec![Content::audio("data", "audio/wav")]);

        let err = validate_interaction_request(&req).unwrap_err();
        assert!(err.to_string().contains("audio input"));
    }

    #[test]
    fn test_antigravity_rejects_video_input() {
        let mut req = base_request();
        req.agent = Some("antigravity-preview-05-2026".to_string());
        req.input = Input::Content(vec![Content::video_uri("https://example.com/video.mp4")]);

        let err = validate_interaction_request(&req).unwrap_err();
        assert!(err.to_string().contains("video input"));
    }

    #[test]
    fn test_antigravity_rejects_document_input() {
        let mut req = base_request();
        req.agent = Some("antigravity-preview-05-2026".to_string());
        req.input = Input::Content(vec![Content::document("data", "application/pdf")]);

        let err = validate_interaction_request(&req).unwrap_err();
        assert!(err.to_string().contains("document input"));
    }

    #[test]
    fn test_deep_research_requires_background() {
        let mut req = base_request();
        req.agent = Some("deep-research-preview-04-2026".to_string());

        let err = validate_interaction_request(&req).unwrap_err();
        assert!(err.to_string().contains("requires background execution"));
    }

    #[test]
    fn test_deep_research_accepts_background_true() {
        let mut req = base_request();
        req.agent = Some("deep-research-preview-04-2026".to_string());
        req.background = Some(true);

        assert!(validate_interaction_request(&req).is_ok());
    }

    #[test]
    fn test_antigravity_valid_request() {
        let mut req = base_request();
        req.agent = Some("antigravity-preview-05-2026".to_string());
        req.store = Some(true);

        assert!(validate_interaction_request(&req).is_ok());
    }

    #[test]
    fn test_no_agent_no_model_passes() {
        let req = base_request();
        assert!(validate_interaction_request(&req).is_ok());
    }
}
