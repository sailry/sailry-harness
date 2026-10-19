//! Public suggestions use one cancellable cache per Node, without configuration storage.
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, conversation::presets::Catalog};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const URL: &str = "https://raw.githubusercontent.com/sailry/sailry-harness/main/assets/catalogs/model-presets.json";
const BUNDLED: &[u8] = include_bytes!("../../../../assets/catalogs/model-presets.json");
const BYTE_LIMIT: usize = 1024 * 1024;
const TTL: Duration = Duration::from_secs(15 * 60);
const FAILURE_TTL: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(crate) struct Directory {
    document: Mutex<Option<Document>>,
    #[cfg(any(test, feature = "test-support"))]
    fixture: std::sync::RwLock<Option<String>>,
}

struct Document {
    source: String,
    catalog: Catalog,
    unavailable: bool,
    attempted: Instant,
}

impl Directory {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn fixture(&self, endpoint: &str) -> Result<(), Fault> {
        let url = url::Url::parse(endpoint).map_err(|_| invalid())?;
        if url.scheme() != "http"
            || !matches!(url.host_str(), Some("127.0.0.1" | "[::1]"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        *self.fixture.write().map_err(|_| unavailable())? = Some(url.to_string());
        Ok(())
    }

    pub(crate) async fn read(&self, stop: &CancellationToken) -> Result<(Catalog, bool), Fault> {
        #[cfg(any(test, feature = "test-support"))]
        let source = self
            .fixture
            .read()
            .map_err(|_| unavailable())?
            .clone()
            .unwrap_or_else(|| URL.into());
        #[cfg(not(any(test, feature = "test-support")))]
        let source = URL.to_owned();
        self.cached(&source, stop, download(&source)).await
    }

    async fn cached(
        &self,
        source: &str,
        stop: &CancellationToken,
        load: impl std::future::Future<Output = Result<Catalog, Fault>>,
    ) -> Result<(Catalog, bool), Fault> {
        let mut document = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(cancelled()),
            document = self.document.lock() => document,
        };
        if let Some(current) = document.as_ref().filter(|current| current.source == source) {
            let ttl = if current.unavailable {
                FAILURE_TTL
            } else {
                TTL
            };
            if current.attempted.elapsed() < ttl {
                return Ok((current.catalog.clone(), current.unavailable));
            }
        }
        let result = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(cancelled()),
            result = load => result,
        };
        let (catalog, unavailable) = match result {
            Ok(catalog) => (catalog, false),
            Err(_) => (
                document
                    .as_ref()
                    .filter(|current| current.source == source)
                    .map(|current| current.catalog.clone())
                    .unwrap_or_else(bundled),
                true,
            ),
        };
        *document = Some(Document {
            source: source.to_owned(),
            catalog: catalog.clone(),
            unavailable,
            attempted: Instant::now(),
        });
        Ok((catalog, unavailable))
    }
}

async fn download(source: &str) -> Result<Catalog, Fault> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| unavailable())?;
    let mut response = client
        .get(source)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| unavailable())?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > BYTE_LIMIT as u64)
    {
        return Err(unavailable());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
        if chunk.len() > BYTE_LIMIT.saturating_sub(bytes.len()) {
            return Err(unavailable());
        }
        bytes.extend_from_slice(&chunk);
    }
    parse(&bytes)
}

pub(crate) fn bundled() -> Catalog {
    let catalog: Catalog =
        serde_json::from_slice(BUNDLED).expect("bundled channel catalog must decode");
    catalog
}

pub(crate) fn parse(bytes: &[u8]) -> Result<Catalog, Fault> {
    if bytes.len() > BYTE_LIMIT {
        return Err(invalid());
    }
    let mut value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if let Some(object) = value.as_object_mut() {
        object
            .entry("partners")
            .or_insert_with(|| serde_json::json!([]));
    }
    let partners = value
        .get_mut("partners")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(invalid)?;
    let mut ids = BTreeSet::new();
    let mut groups = BTreeMap::new();
    partners.retain(|value| {
        serde_json::from_value::<sailry_protocol::conversation::presets::Preset>(value.clone())
            .is_ok_and(|preset| {
                validate(&preset, false, false).is_ok()
                    && !ids.contains(&preset.id)
                    && group(&preset, &mut groups)
                    && ids.insert(preset.id)
            })
    });
    let mut catalog: Catalog = serde_json::from_value(value).map_err(|_| invalid())?;
    if catalog.version != 1 {
        return Err(invalid());
    }
    // Only suggestions are remote data. Executable builtin policies never come from the feed.
    catalog.builtin.clear();
    catalog.policies.clear();
    let mut ids = BTreeSet::new();
    for preset in &catalog.partners {
        validate(preset, false, false)?;
        if !ids.insert(&preset.id) {
            return Err(invalid());
        }
    }
    Ok(catalog)
}

/// Explicit labels for one source-scoped brand must agree across its routes.
pub(crate) fn group(
    preset: &sailry_protocol::conversation::presets::Preset,
    groups: &mut BTreeMap<String, sailry_protocol::plugin::desktop::Navigation>,
) -> bool {
    let Some((brand, display)) = preset.brand.as_ref().zip(preset.brand_display.as_ref()) else {
        return true;
    };
    if let Some(previous) = groups.get(brand) {
        return previous == display;
    }
    groups.insert(brand.clone(), display.clone());
    true
}

pub(crate) fn validate(
    preset: &sailry_protocol::conversation::presets::Preset,
    builtin: bool,
    package: bool,
) -> Result<(), Fault> {
    use sailry_protocol::conversation::presets::{Auth, Models};
    if preset.id.is_empty()
        || !preset
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        || preset.name.trim().is_empty()
        || preset.routes.is_empty()
        || preset
            .brand
            .as_ref()
            .is_some_and(|brand| brand.trim().is_empty() || brand.chars().any(char::is_control))
        || preset
            .brand_display
            .as_ref()
            .is_some_and(|display| preset.brand.is_none() || !display.valid())
        || (!builtin && matches!(preset.auth, Auth::Builtin { .. }))
        || (!package && (matches!(preset.auth, Auth::Plugin { .. }) || preset.adapter.is_some()))
        || preset.routes.iter().any(|route| {
            (route.protocol == sailry_protocol::conversation::presets::Protocol::Plugin)
                != preset.adapter.is_some()
        })
    {
        return Err(invalid());
    }
    for value in [&preset.homepage, &preset.key_url].into_iter().flatten() {
        public_url(value)?;
    }
    if let Some(logo) = &preset.logo {
        if logo.starts_with("icons/providers/") {
            let name = logo
                .strip_prefix("icons/providers/")
                .unwrap()
                .strip_suffix(".svg")
                .ok_or_else(invalid)?;
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            {
                return Err(invalid());
            }
        } else if package && logo.starts_with("dev.sailry.platform/") {
            crate::files::path::components(logo, false)?;
        } else {
            public_url(logo)?;
        }
    }
    if let Auth::ApiKey { header, prefix } = &preset.auth
        && (header.is_some() || prefix.is_some())
    {
        use reqwest::header::{HeaderName, HeaderValue};
        let name = HeaderName::from_bytes(header.as_deref().unwrap_or("authorization").as_bytes())
            .map_err(|_| invalid())?;
        HeaderValue::from_str(&format!("{}fixture", prefix.as_deref().unwrap_or("")))
            .map_err(|_| invalid())?;
        if matches!(
            name.as_str(),
            "host"
                | "connection"
                | "content-length"
                | "transfer-encoding"
                | "proxy-authorization"
                | "upgrade"
        ) || preset.routes.iter().any(|route| {
            !matches!(
                route.protocol,
                sailry_protocol::conversation::presets::Protocol::OpenAiResponses
                    | sailry_protocol::conversation::presets::Protocol::OpenAiChatCompletions
                    | sailry_protocol::conversation::presets::Protocol::Anthropic
                    | sailry_protocol::conversation::presets::Protocol::Plugin
            )
        }) {
            return Err(invalid());
        }
    }
    for (index, route) in preset.routes.iter().enumerate() {
        if preset.routes[..index]
            .iter()
            .any(|other| other.key() == route.key())
        {
            return Err(invalid());
        }
        if route.endpoint.is_empty() && builtin {
            continue;
        }
        super::endpoint_url(&route.endpoint).map_err(|_| invalid())?;
    }
    if let Models::Static { ids } = &preset.models {
        let mut seen = BTreeSet::new();
        if ids.is_empty()
            || ids
                .iter()
                .any(|id| id.trim().is_empty() || !seen.insert(id))
        {
            return Err(invalid());
        }
    }
    for policy in &preset.policies {
        validate_policy(policy)?;
    }
    Ok(())
}

pub(crate) fn validate_policy(
    policy: &sailry_protocol::conversation::presets::RequestPolicy,
) -> Result<(), Fault> {
    let mut seen = BTreeSet::new();
    if policy.models.is_empty()
        || policy
            .models
            .iter()
            .any(|id| id.trim().is_empty() || !seen.insert(id))
        || policy.body.keys().any(|key| {
            matches!(
                key.as_str(),
                "model"
                    | "messages"
                    | "input"
                    | "tools"
                    | "stream"
                    | "instructions"
                    | "system"
                    | "tool_choice"
                    | "parallel_tool_calls"
                    | "functions"
                    | "function_call"
                    | "response_format"
            )
        })
    {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) fn validate_injection(
    injection: &sailry_protocol::plugin::model_auth::Injection,
) -> Result<(), Fault> {
    let name = reqwest::header::HeaderName::from_bytes(injection.header.as_bytes())
        .map_err(|_| invalid())?;
    let template = injection.template.replace("{key}", "fixture");
    if template.contains(['{', '}'])
        || matches!(
            name.as_str(),
            "host"
                | "connection"
                | "content-length"
                | "transfer-encoding"
                | "proxy-authorization"
                | "upgrade"
        )
    {
        return Err(invalid());
    }
    reqwest::header::HeaderValue::from_str(&template).map_err(|_| invalid())?;
    Ok(())
}

fn public_url(value: &str) -> Result<(), Fault> {
    let url = url::Url::parse(value).map_err(|_| invalid())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> Fault {
    Fault::invalid("invalid channel preset declaration")
}
fn unavailable() -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        "channel preset catalog is unavailable",
    )
}
fn cancelled() -> Fault {
    Fault::new(
        ErrorCode::Cancelled,
        "channel preset catalog read cancelled",
    )
}

#[cfg(test)]
mod tests;
