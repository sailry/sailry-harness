//! Declarative authentication; all HTTP and private values remain on the Node.
use super::{desktop::Navigation, model_adapter::Script};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub flow: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub id: String,
    /// Stable account kind handled by this flow; private credential formats remain unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authentication: Option<crate::Authentication>,
    pub display: Navigation,
    pub flow: Flow,
    #[serde(default)]
    pub client: Client,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<Mapping>,
    #[serde(default)]
    pub inject: Vec<Injection>,
    #[serde(default)]
    pub refresh: Option<Refresh>,
    #[serde(default)]
    pub exchange: Option<Transformation>,
    /// Named read-only resources. Grant headers are injected only by the Node.
    #[serde(default)]
    pub requests: BTreeMap<String, Endpoint>,
    /// Named durable operations. Only the Node can inject the provider grant.
    #[serde(default)]
    pub mutations: BTreeMap<String, Endpoint>,
}

/// A package can decode resources without receiving native account credentials.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub authentication: crate::Authentication,
    pub requests: BTreeMap<String, Endpoint>,
}

impl Resource {
    pub fn valid(&self) -> bool {
        matches!(
            self.authentication,
            crate::Authentication::ChatGpt
                | crate::Authentication::ChatGptOAuth
                | crate::Authentication::ClaudeOAuth
        ) && !self.requests.is_empty()
            && self.requests.len() <= 32
            && self
                .requests
                .iter()
                .all(|(name, endpoint)| named_endpoint(name, endpoint))
    }
}

fn named_endpoint(name: &str, endpoint: &Endpoint) -> bool {
    super::host::valid_handler(name)
        && endpoint.valid()
        && !endpoint.url.contains('{')
        && !endpoint.url.contains('}')
        && endpoint
            .headers
            .values()
            .all(|value| !value.contains('{') && !value.contains('}'))
}

impl Declaration {
    pub fn valid(&self) -> bool {
        super::model_adapter::identifier(&self.id)
            && !self.display.label.trim().is_empty()
            && self.flow.valid()
            && self.client.valid()
            && match &self.flow {
                Flow::Script { .. } => match self.authentication {
                    Some(
                        crate::Authentication::ChatGpt
                        | crate::Authentication::ChatGptOAuth
                        | crate::Authentication::ClaudeOAuth,
                    ) => self.token.is_none() && self.inject.is_empty(),
                    None => {
                        self.token.as_ref().is_some_and(Mapping::valid) && !self.inject.is_empty()
                    }
                    _ => false,
                },
                _ => {
                    self.authentication.is_none()
                        && self.token.as_ref().is_some_and(Mapping::valid)
                        && !self.inject.is_empty()
                }
            }
            && self
                .inject
                .iter()
                .all(|item| !item.header.trim().is_empty() && !item.template.is_empty())
            && self
                .refresh
                .as_ref()
                .is_none_or(|refresh| refresh.request.valid() && refresh.token.valid())
            && self
                .requests
                .iter()
                .chain(&self.mutations)
                .all(|(name, endpoint)| named_endpoint(name, endpoint))
            && self.exchange.as_ref().is_none_or(|exchange| {
                exchange.script.valid() && super::host::valid_handler(&exchange.handler)
            })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Flow {
    /// Pure package steps use the existing private login HTTP, callback and cancellation owners.
    Script {
        script: Script,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        refresh: bool,
        handler: String,
        requests: BTreeMap<String, Endpoint>,
        callback: Option<Callback>,
        issuer: Option<String>,
    },
    /// A fixed start-and-poll challenge. State remains private to the Node.
    Polling {
        start: Endpoint,
        state: String,
        verification_url: String,
        token: Endpoint,
        success: Condition,
        interval_ms: Option<u64>,
    },
    Pkce {
        authorize: String,
        token: Endpoint,
        redirect: Redirect,
        /// Required for a pasted callback; loopback uses the controller's listener.
        redirect_uri: Option<String>,
    },
    DeviceCode {
        device: Endpoint,
        token: Endpoint,
    },
    TokenExchange {
        token: Endpoint,
    },
}

impl Flow {
    pub fn presentation(&self) -> Presentation {
        match self {
            Self::Script { callback, .. } => Presentation::Script {
                callback: callback.clone(),
            },
            Self::Pkce { redirect, .. } => Presentation::Pkce {
                redirect: *redirect,
            },
            Self::Polling { .. } => Presentation::Polling,
            Self::DeviceCode { .. } => Presentation::DeviceCode,
            Self::TokenExchange { .. } => Presentation::TokenExchange,
        }
    }
    fn valid(&self) -> bool {
        match self {
            Self::Script {
                script,
                refresh: _,
                handler,
                requests,
                callback,
                issuer,
            } => {
                script.valid()
                    && super::host::valid_handler(handler)
                    && !requests.is_empty()
                    && requests.len() <= 32
                    && requests.iter().all(|(name, request)| {
                        super::host::valid_handler(name)
                            && request.valid()
                            && !request.url.contains(['{', '}'])
                    })
                    && callback.as_ref().is_none_or(Callback::valid)
                    && issuer.as_ref().is_none_or(|value| {
                        endpoint(value)
                            && url::Url::parse(value)
                                .is_ok_and(|url| url.path() == "/" && url.query().is_none())
                    })
            }
            Self::Polling {
                start,
                state,
                verification_url,
                token,
                success,
                interval_ms,
            } => {
                start.valid()
                    && token.valid()
                    && pointer(state)
                    && pointer(verification_url)
                    && pointer(&success.pointer)
                    && !success.values.is_empty()
                    && interval_ms.is_none_or(|value| value > 0)
            }
            Self::Pkce {
                authorize,
                token,
                redirect,
                redirect_uri,
            } => {
                endpoint(authorize)
                    && token.valid()
                    && match redirect {
                        Redirect::Loopback => redirect_uri.is_none(),
                        Redirect::Paste => redirect_uri.as_ref().is_some_and(|uri| endpoint(uri)),
                    }
            }
            Self::DeviceCode { device, token } => device.valid() && token.valid(),
            Self::TokenExchange { token } => token.valid(),
        }
    }
}

/// Only the interaction needed by a controller, without private login state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Presentation {
    Script { callback: Option<Callback> },
    Pkce { redirect: Redirect },
    Polling,
    DeviceCode,
    TokenExchange,
}

/// Public interaction metadata; callback values and proof keys remain private.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Callback {
    pub path: String,
    pub redirect_uri: Option<String>,
    pub manual: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_localhost: bool,
}

impl Callback {
    fn valid(&self) -> bool {
        self.path.starts_with('/')
            && self.path.len() <= 256
            && !self.path.contains(['?', '#', '\\'])
            && !self.path.chars().any(char::is_control)
            && self
                .redirect_uri
                .as_ref()
                .is_none_or(|value| self.manual && endpoint(value))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub pointer: String,
    pub values: Vec<serde_json::Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Redirect {
    Loopback,
    Paste,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Client {
    pub client_id: Option<String>,
    pub client_id_setting: Option<String>,
    pub client_secret_setting: Option<String>,
}

impl Client {
    fn valid(&self) -> bool {
        !(self.client_id.is_some() && self.client_id_setting.is_some())
            && [
                &self.client_id,
                &self.client_id_setting,
                &self.client_secret_setting,
            ]
            .into_iter()
            .all(|value| value.as_ref().is_none_or(|value| !value.trim().is_empty()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    pub access: String,
    pub refresh: Option<String>,
    pub expires_in: Option<String>,
    /// JSON pointer to the account email, never a name, subject, account ID or secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

impl Mapping {
    fn valid(&self) -> bool {
        pointer(&self.access)
            && [&self.refresh, &self.expires_in, &self.account_label]
                .into_iter()
                .all(|value| value.as_ref().is_none_or(|value| pointer(value)))
            && self
                .extra
                .iter()
                .all(|(key, value)| super::model_adapter::identifier(key) && pointer(value))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Injection {
    pub header: String,
    pub template: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Refresh {
    pub request: Endpoint,
    pub token: Mapping,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Transformation {
    pub script: Script,
    pub handler: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    /// Shared freshness for an explicitly cached read-only provider resource.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 900000))]
    pub cache_ttl_ms: Option<u32>,
    pub url: String,
    #[serde(default)]
    pub method: Method,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub query: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<Body>,
}

impl Endpoint {
    pub fn valid(&self) -> bool {
        endpoint(&self.url)
            && self
                .cache_ttl_ms
                .is_none_or(|ttl| (1..=900_000).contains(&ttl))
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    Get,
    #[default]
    Post,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Body {
    Form { fields: BTreeMap<String, String> },
    Json { value: serde_json::Value },
}

fn endpoint(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
    })
}

fn pointer(value: &str) -> bool {
    value.starts_with('/')
        && value
            .split('~')
            .skip(1)
            .all(|part| part.starts_with('0') || part.starts_with('1'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_private_script_declarations() {
        let declaration:Declaration=serde_json::from_value(json!({
            "id":"account","authentication":"chat_gpt_oauth","display":{"label":"Account"},
            "flow":{"kind":"script","script":{"entry":"dev.sailry.platform/auth.js"},"handler":"authorize",
                "requests":{"token":{"url":"https://issuer.example/token"}},"callback":null,"issuer":"https://issuer.example"}
        })).unwrap();
        assert!(declaration.valid());
        assert!(
            !serde_json::to_value(&declaration)
                .unwrap()
                .as_object()
                .unwrap()
                .contains_key("token")
        );
        for authentication in [
            crate::Authentication::ApiKey,
            crate::Authentication::Copilot,
            crate::Authentication::Plugin,
        ] {
            let mut invalid = declaration.clone();
            invalid.authentication = Some(authentication);
            assert!(!invalid.valid());
        }
        let mut invalid = declaration.clone();
        if let Flow::Script { requests, .. } = &mut invalid.flow {
            requests.get_mut("token").unwrap().url = "https://{host}/token".into();
        }
        assert!(!invalid.valid());
        let mut plugin = declaration.clone();
        plugin.authentication = None;
        assert!(!plugin.valid());
        plugin.token = Some(Mapping {
            account_label: None,
            access: "/access".into(),
            refresh: None,
            expires_in: None,
            extra: BTreeMap::new(),
        });
        plugin.inject = vec![Injection {
            header: "Authorization".into(),
            template: "Bearer {access}".into(),
        }];
        assert!(plugin.valid());
    }

    #[test]
    fn preserves_omitted_alias_fields() {
        let document = json!({"id":"account","display":{"label":"Account"},"flow":{"kind":"token_exchange","token":{"url":"https://example.test/token"}},"token":{"access":"/access","refresh":null,"expires_in":null},"inject":[{"header":"Authorization","template":"Bearer {access}"}]});
        let declaration: Declaration = serde_json::from_value(document).unwrap();
        assert!(declaration.authentication.is_none());
        assert!(declaration.valid());
        assert!(
            serde_json::to_value(declaration)
                .unwrap()
                .get("authentication")
                .is_none()
        );
    }

    #[test]
    fn bounds_native_resource_declarations() {
        for authentication in ["chat_gpt", "chat_gpt_oauth", "claude_oauth"] {
            let resource: Resource = serde_json::from_value(json!({"authentication":authentication,"requests":{"usage":{"url":"https://example.invalid/usage","query":{"after":"{input.after}"}}}})).unwrap();
            assert!(resource.valid());
            let mut invalid = resource.clone();
            invalid
                .requests
                .get_mut("usage")
                .unwrap()
                .headers
                .insert("X-Input".into(), "{input.header}".into());
            assert!(!invalid.valid());
        }
        for authentication in ["api_key", "copilot", "plugin"] {
            let resource: Resource = serde_json::from_value(json!({"authentication":authentication,"requests":{"usage":{"url":"https://example.invalid/usage"}}})).unwrap();
            assert!(!resource.valid());
        }
        let extension: crate::plugin::Extension =
            serde_json::from_value(json!({"api_version":"v1"})).unwrap();
        assert!(extension.model_resources.is_empty());
        assert!(
            serde_json::to_value(extension)
                .unwrap()
                .get("model_resources")
                .is_none()
        );
    }

    #[test]
    fn keeps_resource_urls_and_headers_fixed() {
        let mut declaration:Declaration=serde_json::from_value(json!({"id":"example","display":{"label":"Example"},"flow":{"kind":"token_exchange","token":{"url":"https://example.invalid/token"}},"token":{"access":"/access","refresh":null,"expires_in":null},"inject":[{"header":"Authorization","template":"Bearer {access}"}],"requests":{"status":{"url":"https://example.invalid/status","query":{"after":"{input.after}"}}}})).unwrap();
        assert!(declaration.valid());
        declaration
            .requests
            .get_mut("status")
            .unwrap()
            .headers
            .insert("X-Input".into(), "{input.header}".into());
        assert!(!declaration.valid());
        declaration
            .requests
            .get_mut("status")
            .unwrap()
            .headers
            .clear();
        declaration.requests.get_mut("status").unwrap().url = "https://{input.host}/status".into();
        assert!(!declaration.valid());
        declaration.requests.clear();
        if let Flow::TokenExchange { token } = &mut declaration.flow {
            token.headers.insert("X-Refresh".into(), "{refresh}".into());
        }
        assert!(
            declaration.valid(),
            "authentication retains its private template contract"
        );
    }

    #[test]
    fn keeps_mutation_endpoints_fixed() {
        let mut declaration: Declaration = serde_json::from_value(json!({
            "id":"example","display":{"label":"Example"},
            "flow":{"kind":"token_exchange","token":{"url":"https://example.invalid/token"}},
            "token":{"access":"/access","refresh":null,"expires_in":null},
            "inject":[{"header":"Authorization","template":"Bearer {access}"}],
            "mutations":{"claim":{"url":"https://example.invalid/claim","method":"POST","body":{"kind":"json","value":{"day":"{input.day}"}}}}
        })).unwrap();
        assert!(declaration.requests.is_empty());
        assert!(declaration.valid());
        declaration
            .mutations
            .get_mut("claim")
            .unwrap()
            .headers
            .insert("X-Input".into(), "{input.header}".into());
        assert!(!declaration.valid());
        declaration
            .mutations
            .get_mut("claim")
            .unwrap()
            .headers
            .clear();
        declaration.mutations.get_mut("claim").unwrap().url = "https://{input.host}/claim".into();
        assert!(!declaration.valid());
    }
    #[test]
    fn preserves_uncached_endpoint_shape() {
        let value = json!({"url":"https://example.test/usage","method":"GET","headers":{},"query":{},"body":null});
        let endpoint: Endpoint = serde_json::from_value(value.clone()).unwrap();
        assert!(endpoint.valid());
        assert_eq!(serde_json::to_value(endpoint).unwrap(), value);
        for ttl in [1, 60000, 180000, 900000] {
            let endpoint: Endpoint = serde_json::from_value(
                json!({"url":"https://example.test/usage","cache_ttl_ms":ttl}),
            )
            .unwrap();
            assert!(endpoint.valid());
        }
        for ttl in [0, 900001] {
            let endpoint: Endpoint = serde_json::from_value(
                json!({"url":"https://example.test/usage","cache_ttl_ms":ttl}),
            )
            .unwrap();
            assert!(!endpoint.valid());
        }
    }
}
