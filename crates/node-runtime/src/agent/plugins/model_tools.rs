//! Package declarations select shared provider capabilities, not a second tool runner.
use super::*;
use sailry_protocol::{
    Authentication,
    conversation::ModelApi,
    plugin::{conversation::Tool as ToolSelection, model_tools::Capability},
};

pub(super) fn names(capability: Capability) -> &'static [&'static str] {
    match capability {
        Capability::WebSearch => &["openai_web_search", "web_search", "google_search"],
    }
}

pub(super) fn bind(invocation: &Invocation) -> Result<Vec<ToolRegistration>, Fault> {
    let assistant = invocation
        .turn
        .config
        .assistant
        .as_ref()
        .map(|binding| crate::plugins::conversation::declaration(binding, &invocation.plugins))
        .transpose()?;
    let mut enabled = BTreeSet::new();
    let mut registrations = Vec::new();
    for package in &invocation.plugins {
        if invocation
            .turn
            .config
            .assistant
            .as_ref()
            .is_some_and(|binding| package.summary.reference() != binding.package)
        {
            continue;
        }
        let Some(extension) = &package.extension else {
            continue;
        };
        for declaration in &extension.model_tools {
            if assistant.is_some_and(|assistant| {
                !assistant.tools.iter().any(|tool| {
                    matches!(tool, ToolSelection::Model { capability } if *capability == declaration.capability)
                })
            }) {
                continue;
            }
            if !enabled.insert(declaration.capability) {
                continue;
            }
            let tool = match declaration.capability {
                Capability::WebSearch => search(invocation)?,
            };
            if let Some(tool) = tool {
                let mut registration = ToolRegistration::managed(tool);
                registration.plugin = Some(package.summary.name.clone());
                registration.display =
                    Some(("web_search".into(), declaration.display.clone().into()));
                registrations.push(registration);
            }
        }
    }
    Ok(registrations)
}

fn search(invocation: &Invocation) -> Result<Option<Arc<dyn adk_core::Tool>>, Fault> {
    let Some(provider) = &invocation.provider else {
        return Ok(None);
    };
    let Some(model) = provider
        .models
        .iter()
        .find(|model| model.id == invocation.turn.config.model)
    else {
        return Ok(None);
    };
    if !model.web_search
        || !crate::providers::search::combines(
            provider.api,
            &model.id,
            invocation.turn.config.effort,
            model.tools,
        )
    {
        return Ok(None);
    }
    match (provider.authentication, provider.api) {
        (Authentication::ApiKey, ModelApi::Responses) => {
            Ok(Some(Arc::new(adk_tool::OpenAIWebSearchTool::new())))
        }
        (Authentication::ApiKey, ModelApi::Anthropic) => {
            Ok(Some(Arc::new(adk_tool::WebSearchTool::new())))
        }
        (Authentication::ApiKey, ModelApi::Gemini) => {
            Ok(Some(Arc::new(adk_tool::GoogleSearchTool::new())))
        }
        _ => Err(Fault::new(
            ErrorCode::NotConfigured,
            "native web search is unavailable for this provider API",
        )),
    }
}
