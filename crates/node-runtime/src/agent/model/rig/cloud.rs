//! Routing derived from Goose 14741e9a azure_foundry.rs (Apache-2.0).
//! See third_party_licenses/goose-Apache-2.0.txt.

use super::super::types::{Error, ErrorKind};
use reqwest::header::HeaderMap;
use sailry_protocol::conversation::{ModelApi, Provider};
use serde_json::Value;
use std::{sync::OnceLock, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Maas,
    Resource,
    Project,
}

pub(super) struct Route {
    pub api: ModelApi,
    pub endpoint: url::Url,
    pub underlying: String,
}

pub(super) fn kind(endpoint: &url::Url) -> Kind {
    if endpoint.path().contains("/api/projects/") {
        Kind::Project
    } else if endpoint
        .host_str()
        .is_some_and(|host| host.ends_with(".services.ai.azure.com"))
    {
        Kind::Resource
    } else {
        Kind::Maas
    }
}

pub(super) async fn resolve(
    client: &reqwest::Client,
    provider: &Provider,
    model: &str,
    key: &str,
    headers: &HeaderMap,
) -> Result<Route, Error> {
    let mut endpoint =
        crate::providers::endpoint_url(&provider.endpoint).map_err(super::errors::local)?;
    let endpoint_kind = kind(&endpoint);
    if endpoint_kind == Kind::Maas {
        let path = endpoint.path().trim_end_matches("/chat/completions");
        let path = path.strip_suffix("/v1").unwrap_or(path);
        let path = format!("{}/v1", path.trim_end_matches('/'));
        endpoint.set_path(&path);
        replace_query(&mut endpoint, "api-version", "2024-05-01-preview");
        return Ok(Route {
            api: ModelApi::ChatCompletions,
            endpoint,
            underlying: model.to_owned(),
        });
    }
    let deployment = if endpoint_kind == Kind::Project {
        tokio::time::timeout(
            Duration::from_secs(5),
            lookup(client, provider, model, key, headers, &endpoint),
        )
        .await
        .ok()
        .and_then(Result::ok)
        .flatten()
    } else {
        None
    };
    let underlying = deployment
        .as_ref()
        .map(|deployment| deployment.model.as_str())
        .unwrap_or(model);
    let publisher = deployment
        .as_ref()
        .and_then(|deployment| deployment.publisher.as_deref());
    let anthropic = publisher.map_or_else(
        || underlying.to_ascii_lowercase().starts_with("claude"),
        |publisher| publisher.eq_ignore_ascii_case("anthropic"),
    );
    let openai = publisher.map_or_else(
        || responses_model(underlying),
        |publisher| publisher.eq_ignore_ascii_case("openai"),
    );
    let api = if anthropic {
        let root = endpoint
            .path()
            .split("/api/projects/")
            .next()
            .unwrap_or("")
            .trim_end_matches('/');
        let path = format!("{root}/anthropic");
        endpoint.set_path(&path);
        ModelApi::Anthropic
    } else {
        let path = format!("{}/openai/v1", endpoint.path().trim_end_matches('/'));
        endpoint.set_path(&path);
        if openai && responses_model(underlying) {
            ModelApi::Responses
        } else {
            ModelApi::ChatCompletions
        }
    };
    Ok(Route {
        api,
        endpoint,
        underlying: underlying.to_owned(),
    })
}

struct Deployment {
    model: String,
    publisher: Option<String>,
}

async fn lookup(
    client: &reqwest::Client,
    provider: &Provider,
    model: &str,
    key: &str,
    supplied: &HeaderMap,
    endpoint: &url::Url,
) -> Result<Option<Deployment>, Error> {
    let credentials = crate::providers::request::Credentials::ApiKey(
        (!key.is_empty()).then(|| sailry_protocol::Secret::new(key.to_owned())),
    );
    let mut headers =
        crate::providers::request::headers(provider, &credentials).map_err(super::errors::local)?;
    headers.extend(supplied.clone());
    let mut next = endpoint.clone();
    let path = format!("{}/deployments", endpoint.path().trim_end_matches('/'));
    next.set_path(&path);
    if !next.query_pairs().any(|(name, _)| name == "api-version") {
        next.query_pairs_mut().append_pair("api-version", "v1");
    }
    loop {
        if next.origin() != endpoint.origin() {
            return Err(Error::new(
                ErrorKind::InvalidResponse,
                "deployment pagination crossed the configured provider origin",
            ));
        }
        let response = client
            .get(next.clone())
            .headers(headers.clone())
            .send()
            .await
            .map_err(|_| Error::new(ErrorKind::Network, "deployment lookup failed"))?;
        if !response.status().is_success() {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "deployment lookup was rejected",
            ));
        }
        let body: Value = response.json().await.map_err(|_| {
            Error::new(
                ErrorKind::InvalidResponse,
                "deployment lookup returned invalid data",
            )
        })?;
        if let Some(item) = body["value"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|item| item["name"].as_str() == Some(model))
        {
            return Ok(Some(Deployment {
                model: item["modelName"].as_str().unwrap_or(model).to_owned(),
                publisher: item["modelPublisher"].as_str().map(str::to_owned),
            }));
        }
        let Some(link) = body["nextLink"].as_str() else {
            return Ok(None);
        };
        next = next.join(link).map_err(|_| {
            Error::new(
                ErrorKind::InvalidResponse,
                "deployment pagination returned an invalid URL",
            )
        })?;
        if !next.query_pairs().any(|(name, _)| name == "api-version") {
            next.query_pairs_mut().append_pair("api-version", "v1");
        }
    }
}

pub(super) fn responses_model(model: &str) -> bool {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN
        .get_or_init(|| {
            regex::Regex::new(r"(?i)(?:^|[-/])(?:o\d+(?:$|-)|gpt-(?:5|6)(?:$|[-.]))")
                .expect("static model routing pattern")
        })
        .is_match(model)
}

pub(super) fn replace_query(endpoint: &mut url::Url, name: &str, value: &str) {
    let mut query: Vec<_> = endpoint
        .query_pairs()
        .filter(|(key, _)| key != name)
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    query.push((name.to_owned(), value.to_owned()));
    endpoint.set_query(None);
    endpoint.query_pairs_mut().extend_pairs(query);
}
