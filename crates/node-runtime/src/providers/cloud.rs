//! Validate cloud routing before saving or invoking a frozen configuration.
use super::*;
use sailry_protocol::conversation::{Provider, cloud::Options};

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
        (ModelApi::AzureOpenAi, _) => false,
        (_, None) => true,
        _ => false,
    };
    if !valid {
        return Err(invalid("cloud routing options do not match the provider"));
    }
    Ok(())
}

pub(crate) fn deployment(api: ModelApi) -> bool {
    matches!(api, ModelApi::AzureOpenAi | ModelApi::AzureAi)
}
