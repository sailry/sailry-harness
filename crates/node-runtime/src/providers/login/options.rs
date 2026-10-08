use super::*;
use reqwest::header::HeaderValue;

pub(crate) fn capture(provider: &mut Provider) -> Result<(), Fault> {
    if matches!(
        provider.authentication,
        Authentication::ChatGpt | Authentication::ChatGptOAuth | Authentication::Copilot
    ) {
        provider.oauth = Some(effective(provider)?);
    }
    Ok(())
}

pub(crate) fn effective(provider: &Provider) -> Result<Options, Fault> {
    let mut options = provider
        .oauth
        .clone()
        .or_else(|| Options::defaults(provider.authentication))
        .ok_or_else(|| sailry_protocol::Fault::invalid("provider does not use OAuth settings"))?;
    if let Options::ChatGptOAuth {
        catalog_version, ..
    } = &mut options
    {
        catalog_version.get_or_insert_with(|| {
            sailry_protocol::conversation::oauth::DEFAULT_CATALOG_VERSION.into()
        });
    }
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
        Options::ChatGpt { user_agent, .. } | Options::ChatGptOAuth { user_agent, .. } => {
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
        HeaderValue::from_str(field)
            .map_err(|_| sailry_protocol::Fault::invalid("OAuth header value is invalid"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_catalog_default() {
        let provider = Provider {
            id: sailry_protocol::ProviderId::new(),
            revision: 1,
            name: "Catalog fixture".into(),
            api: ModelApi::Responses,
            authentication: Authentication::ChatGptOAuth,
            endpoint: CODEX_ENDPOINT.into(),
            enabled: true,
            models: vec![],
            default_model: String::new(),
            credential: None,
            options: None,
            oauth: Some(Options::ChatGptOAuth {
                catalog_version: None,
                user_agent: "Sailry/0.1".into(),
            }),
        };
        let mut captured = provider.clone();
        capture(&mut captured).unwrap();
        assert_eq!(
            captured.oauth,
            Options::defaults(Authentication::ChatGptOAuth)
        );
        let Some(Options::ChatGptOAuth {
            catalog_version, ..
        }) = captured.oauth
        else {
            unreachable!()
        };
        assert_eq!(
            catalog_version.as_deref(),
            Some(sailry_protocol::conversation::oauth::DEFAULT_CATALOG_VERSION)
        );
        assert!(matches!(
            provider.oauth,
            Some(Options::ChatGptOAuth {
                catalog_version: None,
                ..
            })
        ));
    }

    #[test]
    fn preserves_siwc_capture_without_catalog_default() {
        for catalog_version in [None, Some("0.161.0".into())] {
            let mut provider = Provider {
                id: sailry_protocol::ProviderId::new(),
                revision: 1,
                name: "SIWC fixture".into(),
                api: ModelApi::Responses,
                authentication: Authentication::ChatGpt,
                endpoint: CHATGPT_ENDPOINT.into(),
                enabled: true,
                models: vec![],
                default_model: String::new(),
                credential: None,
                options: None,
                oauth: Some(Options::ChatGpt {
                    catalog_version,
                    user_agent: "CapturedAgent/1".into(),
                }),
            };
            let retained = provider.oauth.clone();
            capture(&mut provider).unwrap();
            assert_eq!(provider.oauth, retained);
            assert_eq!(provider.oauth.as_ref().unwrap().catalog_version(), None);
        }
    }
}
