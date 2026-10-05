use super::*;
use reqwest::header::HeaderValue;

pub(crate) fn capture(provider: &mut Provider) -> Result<(), Fault> {
    if matches!(
        provider.authentication,
        Authentication::ChatGpt | Authentication::Copilot
    ) {
        provider.oauth = Some(effective(provider)?);
    }
    Ok(())
}

pub(crate) fn effective(provider: &Provider) -> Result<Options, Fault> {
    let options = provider
        .oauth
        .clone()
        .or_else(|| Options::defaults(provider.authentication))
        .ok_or_else(|| invalid("provider does not use OAuth settings"))?;
    validate(provider.authentication, provider.api, &options)?;
    Ok(options)
}

pub(super) fn validate(
    authentication: Authentication,
    api: ModelApi,
    options: &Options,
) -> Result<(), Fault> {
    options.validate(authentication, api)?;
    let fields = match options {
        Options::ChatGpt {
            catalog_version,
            user_agent,
        } => {
            semver::Version::parse(catalog_version)
                .map_err(|_| invalid("OAuth catalog version is invalid"))?;
            vec![user_agent.as_str()]
        }
        Options::Copilot {
            user_agent,
            editor_version,
            editor_plugin_version,
        } => {
            vec![
                user_agent.as_str(),
                editor_version.as_str(),
                editor_plugin_version.as_str(),
            ]
        }
    };
    for field in fields {
        HeaderValue::from_str(field).map_err(|_| invalid("OAuth header value is invalid"))?;
    }
    Ok(())
}
