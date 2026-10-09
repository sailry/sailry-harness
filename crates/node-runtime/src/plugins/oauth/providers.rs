use super::*;
use reqwest::Url;
use sailry_protocol::plugin::oauth::{Configuration, Provider};

pub(super) fn scopes(declaration: &Declaration) -> Result<String, Fault> {
    let allowed: &[&str] = match declaration.provider {
        Provider::Google => &[
            "https://www.googleapis.com/auth/gmail.readonly",
            "https://www.googleapis.com/auth/calendar.events.readonly",
            "https://www.googleapis.com/auth/drive.readonly",
        ],
        Provider::Microsoft => &["Mail.Read", "Calendars.Read", "Files.Read"],
        Provider::Dingtalk => &["openid", "corpid"],
    };
    if declaration.scopes.is_empty()
        || declaration.scopes.len() > allowed.len()
        || declaration.scopes.iter().enumerate().any(|(index, scope)| {
            !allowed.contains(&scope.as_str()) || declaration.scopes[..index].contains(scope)
        })
    {
        return Err(Fault::invalid("unsupported plugin OAuth scopes"));
    }
    let mut scopes = declaration.scopes.join(" ");
    if declaration.provider == Provider::Microsoft {
        scopes.push_str(" offline_access");
    }
    Ok(scopes)
}

pub(super) fn binding(provider: Provider, origin: &str, header: &str, prefix: &str) -> bool {
    match provider {
        Provider::Google => {
            matches!(
                origin,
                "https://gmail.googleapis.com/" | "https://www.googleapis.com/"
            ) && header == "authorization"
                && prefix == "Bearer "
        }
        Provider::Microsoft => {
            origin == "https://graph.microsoft.com/"
                && header == "authorization"
                && prefix == "Bearer "
        }
        Provider::Dingtalk => {
            origin == "https://api.dingtalk.com/"
                && header == "x-acs-dingtalk-access-token"
                && prefix.is_empty()
        }
    }
}

pub(super) fn endpoints(
    provider: Provider,
    configuration: &Configuration,
) -> Result<(Url, Url), Fault> {
    if configuration.client_id.is_empty()
        || configuration.client_id.len() > 2048
        || configuration.client_id.chars().any(char::is_control)
        || configuration.client_secret.as_ref().is_some_and(|secret| {
            secret.expose().is_empty()
                || secret.expose().len() > 4096
                || secret.expose().chars().any(char::is_control)
        })
    {
        return Err(Fault::invalid("invalid plugin OAuth client configuration"));
    }
    if provider != Provider::Microsoft && configuration.tenant.is_some() {
        return Err(Fault::invalid(
            "OAuth tenant is only supported by Microsoft",
        ));
    }
    let urls = match provider {
        Provider::Google => (
            "https://accounts.google.com/o/oauth2/v2/auth".into(),
            "https://oauth2.googleapis.com/token".into(),
        ),
        Provider::Microsoft => {
            if configuration.client_secret.is_some() {
                return Err(Fault::invalid(
                    "Microsoft desktop login uses a public client",
                ));
            }
            let tenant = configuration.tenant.as_deref().unwrap_or("common");
            if tenant.is_empty()
                || tenant.len() > 253
                || !tenant
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
            {
                return Err(Fault::invalid("invalid Microsoft OAuth tenant"));
            }
            (
                format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/authorize"),
                format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token"),
            )
        }
        Provider::Dingtalk => {
            if configuration.client_secret.is_none() {
                return Err(Fault::invalid(
                    "DingTalk OAuth requires an application secret",
                ));
            }
            (
                "https://login.dingtalk.com/oauth2/auth".into(),
                "https://api.dingtalk.com/v1.0/oauth2/userAccessToken".into(),
            )
        }
    };
    Ok((
        Url::parse(&urls.0).map_err(|_| unavailable())?,
        Url::parse(&urls.1).map_err(|_| unavailable())?,
    ))
}
