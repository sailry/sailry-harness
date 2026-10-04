//! Validate cloud routing before saving or invoking a frozen configuration.
use super::*;
use sailry_protocol::{
    Authentication,
    conversation::{Provider, cloud::Options},
};

fn segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && !matches!(value, "." | "..")
}

pub(crate) fn validate(provider: &Provider) -> Result<(), Fault> {
    let valid = match (provider.api, &provider.options) {
        (ModelApi::AzureOpenAi, Some(Options::AzureOpenAi { api_version })) => {
            segment(api_version) && provider.models.iter().all(|model| segment(&model.id))
        }
        (ModelApi::Bedrock, Some(Options::Bedrock { region })) => segment(region),
        (ModelApi::Vertex, Some(Options::Vertex { project, location })) => {
            segment(project) && segment(location)
        }
        (ModelApi::AzureOpenAi | ModelApi::Bedrock | ModelApi::Vertex, _) => false,
        (_, None) => true,
        _ => false,
    };
    if !valid {
        return Err(invalid("cloud routing options do not match the provider"));
    }
    if provider.authentication == Authentication::Host
        && (!matches!(provider.api, ModelApi::Bedrock | ModelApi::Vertex)
            || provider.credential.is_some())
    {
        return Err(invalid(
            "host identity requires a cloud provider without a stored credential",
        ));
    }
    Ok(())
}

pub(crate) fn deployment(api: ModelApi) -> bool {
    matches!(
        api,
        ModelApi::AzureOpenAi | ModelApi::AzureAi | ModelApi::Bedrock | ModelApi::Vertex
    )
}
