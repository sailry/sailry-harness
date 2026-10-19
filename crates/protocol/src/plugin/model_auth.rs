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
    pub display: Navigation,
    pub flow: Flow,
    #[serde(default)]
    pub client: Client,
    #[serde(default)]
    pub scopes: Vec<String>,
    pub token: Mapping,
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

impl Declaration {
    pub fn valid(&self) -> bool {
        super::model_adapter::identifier(&self.id)
            && !self.display.label.trim().is_empty()
            && self.flow.valid()
            && self.client.valid()
            && self.token.valid()
            && !self.inject.is_empty()
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
                .all(|(name, endpoint)| {
                    super::host::valid_handler(name)
                        && endpoint.valid()
                        && !endpoint.url.contains('{')
                        && !endpoint.url.contains('}')
                        && endpoint
                            .headers
                            .values()
                            .all(|value| !value.contains('{') && !value.contains('}'))
                })
            && self.exchange.as_ref().is_none_or(|exchange| {
                exchange.script.valid() && super::host::valid_handler(&exchange.handler)
            })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Flow {
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Presentation {
    Pkce { redirect: Redirect },
    Polling,
    DeviceCode,
    TokenExchange,
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
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

impl Mapping {
    fn valid(&self) -> bool {
        pointer(&self.access)
            && [&self.refresh, &self.expires_in]
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
}
