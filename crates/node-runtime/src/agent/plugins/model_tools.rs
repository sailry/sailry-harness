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

pub(super) fn bind(
    ingress: &Ingress,
    invocation: &Invocation,
) -> Result<Vec<ToolRegistration>, Fault> {
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
                Capability::WebSearch => search(ingress, invocation),
            };
            if let Some(tool) = tool {
                let mut registration = ToolRegistration::managed(tool);
                registration.capture(registration.tool.declaration());
                registration.plugin = Some(package.summary.name.clone());
                registration.display =
                    Some(("web_search".into(), declaration.display.clone().into()));
                registrations.push(registration);
            }
        }
    }
    Ok(registrations)
}

pub(super) fn preferred(ingress: &Ingress, invocation: &Invocation) -> bool {
    if !ingress.search.settings().prefer_native || !supported(invocation) {
        return false;
    }
    let assistant = invocation.turn.config.assistant.as_ref();
    let declaration = assistant.and_then(|binding| {
        crate::plugins::conversation::declaration(binding, &invocation.plugins).ok()
    });
    invocation.plugins.iter().any(|package| {
        assistant.is_none_or(|binding| package.summary.reference() == binding.package)
            && package.extension.as_ref().is_some_and(|extension| {
                extension
                    .model_tools
                    .iter()
                    .any(|tool| tool.capability == Capability::WebSearch)
            })
            && declaration.is_none_or(|assistant| {
                assistant.tools.iter().any(|tool| {
                    matches!(
                        tool,
                        ToolSelection::Model {
                            capability: Capability::WebSearch
                        }
                    )
                })
            })
    })
}

fn supported(invocation: &Invocation) -> bool {
    let Some(provider) = &invocation.provider else {
        return false;
    };
    let Some(model) = provider
        .models
        .iter()
        .find(|model| model.id == invocation.turn.config.model)
    else {
        return false;
    };
    if !model.web_search
        || !crate::providers::search::combines(
            provider.api,
            &model.id,
            invocation.turn.config.effort,
            model.tools,
        )
    {
        return false;
    }
    matches!(
        (provider.authentication, provider.api),
        (
            Authentication::ApiKey,
            ModelApi::Responses | ModelApi::Anthropic | ModelApi::Gemini
        ) | (Authentication::ChatGptOAuth, ModelApi::Responses)
    )
}

fn search(ingress: &Ingress, invocation: &Invocation) -> Option<Arc<dyn adk_core::Tool>> {
    if !preferred(ingress, invocation) {
        return None;
    }
    let provider = invocation.provider.as_ref()?;
    match (provider.authentication, provider.api) {
        (Authentication::ApiKey | Authentication::ChatGptOAuth, ModelApi::Responses) => {
            Some(Arc::new(adk_tool::OpenAIWebSearchTool::new()))
        }
        (Authentication::ApiKey, ModelApi::Anthropic) => {
            Some(Arc::new(adk_tool::WebSearchTool::new()))
        }
        (Authentication::ApiKey, ModelApi::Gemini) => {
            Some(Arc::new(adk_tool::GoogleSearchTool::new()))
        }
        _ => None,
    }
}
