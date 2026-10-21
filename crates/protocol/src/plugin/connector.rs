//! Package-declared service connections. Credentials remain on the execution Node.
use super::{Reference, desktop::Navigation};
use crate::{Fault, RequestId, Secret};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub type Id = RequestId;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub id: String,
    pub display: Navigation,
    pub accounts: Accounts,
    pub origins: Vec<String>,
    pub auth: Vec<Auth>,
    #[serde(default)]
    pub fields: Vec<Field>,
    pub inject: Vec<Injection>,
    #[serde(default)]
    pub identity: Option<IdentityRequest>,
    #[serde(default)]
    pub health: Option<HealthRequest>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Accounts {
    Single,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub name: String,
    pub title: String,
    #[serde(default)]
    pub locales: BTreeMap<String, String>,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub help: Option<String>,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Oauth2,
    Application,
    Secret,
    McpOauth,
    LocalCli,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Auth {
    Oauth2 {
        flow: Flow,
        authorize: String,
        token: String,
        #[serde(default)]
        device: Option<String>,
        client: Client,
        #[serde(default)]
        scopes: Vec<String>,
        #[serde(default)]
        refresh: bool,
    },
    Application {
        fields: ClientFields,
        token: Token,
        #[serde(default)]
        registration: Option<Box<Registration>>,
    },
    Secret {
        fields: Vec<String>,
    },
    McpOauth {
        server: String,
        #[serde(default)]
        client: Option<Client>,
    },
    LocalCli {
        command: String,
        #[serde(default)]
        env: BTreeMap<String, String>,
        status: CliStatus,
        token: CliToken,
        timeout_ms: u64,
        max_output: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Flow {
    Pkce,
    DeviceCode,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum Client {
    Bundled { bundled: PublicClient },
    Fields { fields: ClientFields },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicClient {
    pub client_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClientFields {
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Token {
    pub url: String,
    pub body: BTreeMap<String, String>,
    pub access: String,
    pub expires_in: String,
    #[serde(default)]
    pub form: bool,
}

/// Registration steps use the same private declarative runner as account login.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub flow: super::model_auth::Flow,
    pub token: super::model_auth::Mapping,
    pub issues: ClientFields,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CliStatus {
    pub argv: Vec<String>,
    pub account: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<super::model_auth::Condition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CliToken {
    pub argv: Vec<String>,
    pub parse: Parse,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Parse {
    Line,
    Json(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Injection {
    #[serde(default)]
    pub origin: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default)]
    pub env: Option<String>,
    #[serde(default)]
    pub prefix: String,
    /// Selects a secret when this method declares more than one protected field.
    #[serde(default)]
    pub field: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdentityRequest {
    pub request: String,
    pub name: String,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HealthRequest {
    pub request: String,
    pub ok: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Type {
    pub package: Reference,
    pub declaration: Declaration,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub name: String,
    pub id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Connected,
    NotConnected,
    Connecting,
    Expired,
    NeedsConfirmation,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub id: Id,
    pub package: Reference,
    pub connector: String,
    pub method: Kind,
    pub revision: u64,
    pub enabled: bool,
    pub state: State,
    #[serde(default)]
    pub can_disconnect: bool,
    pub identity: Option<Identity>,
    pub expires_at_ms: Option<u64>,
    pub checked_at_ms: Option<u64>,
    pub fields: BTreeMap<String, String>,
    pub configured: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consent: Option<Consent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Consent {
    pub changes: Vec<Change>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    Origins {
        previous: Vec<String>,
        current: Vec<String>,
    },
    Scopes {
        previous: Vec<String>,
        current: Vec<String>,
    },
    Client {
        previous: String,
        current: String,
    },
    Flow {
        previous: String,
        current: String,
    },
    Targets {
        previous: Vec<String>,
        current: Vec<String>,
    },
    Mcp {
        servers: Vec<String>,
    },
    Source,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capture {
    pub id: Id,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    pub id: RequestId,
    pub connection: Id,
    pub package: Reference,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum LoginState {
    Starting,
    Device {
        code: String,
        url: String,
        expires_at_ms: u64,
    },
    Browser {
        url: String,
        expires_at_ms: u64,
    },
    Exchanging,
    Connected(Connection),
    Failed(Fault),
    Cancelled,
}

impl LoginState {
    pub fn active(&self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Device { .. } | Self::Browser { .. } | Self::Exchanging
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginUpdate {
    pub attempt: Attempt,
    pub revision: u64,
    pub state: LoginState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Changes {
    pub cursor: u64,
    pub types: Vec<Type>,
    pub connections: Vec<Connection>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Input {
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    #[serde(default)]
    pub secrets: BTreeMap<String, Secret>,
    #[serde(default)]
    pub redirect: Option<String>,
    #[serde(default)]
    pub registration: bool,
}

impl Auth {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Oauth2 { .. } => Kind::Oauth2,
            Self::Application { .. } => Kind::Application,
            Self::Secret { .. } => Kind::Secret,
            Self::McpOauth { .. } => Kind::McpOauth,
            Self::LocalCli { .. } => Kind::LocalCli,
        }
    }
    pub fn consent(&self) -> bool {
        matches!(
            self,
            Self::Oauth2 { .. } | Self::Application { .. } | Self::McpOauth { .. }
        )
    }
}

impl Declaration {
    pub fn valid(&self) -> bool {
        self.validate().is_ok()
    }
    pub fn validate(&self) -> Result<(), Fault> {
        let invalid = || Fault::invalid("invalid connector declaration");
        if !identifier(&self.id)
            || !self.display.valid()
            || self.origins.is_empty()
            || self.origins.len() > 16
            || self.auth.is_empty()
            || self.auth.len() > 4
            || self.fields.len() > 8
            || self.inject.is_empty()
            || self.inject.len() > 32
        {
            return Err(invalid());
        }
        let origins = self.origins.iter().collect::<BTreeSet<_>>();
        if origins.len() != self.origins.len() || self.origins.iter().any(|value| !origin(value)) {
            return Err(invalid());
        }
        let fields = self
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<BTreeSet<_>>();
        if fields.len() != self.fields.len()
            || self.fields.iter().any(|field| {
                !identifier(&field.name)
                    || field.title.trim().is_empty()
                    || field.title.len() > 256
                    || field
                        .locales
                        .values()
                        .any(|v| v.trim().is_empty() || v.len() > 256)
                    || field
                        .default
                        .as_ref()
                        .is_some_and(|v| field.secret || v.len() > 2048)
                    || field.help.as_ref().is_some_and(|value| {
                        !url::Url::parse(value).is_ok_and(|url| {
                            url.scheme() == "https"
                                && url.username().is_empty()
                                && url.password().is_none()
                                && url.fragment().is_none()
                        })
                    })
            })
        {
            return Err(invalid());
        }
        let mut targets = BTreeSet::new();
        for item in &self.inject {
            let target = match (&item.origin, &item.server) {
                (Some(value), None) if origins.contains(value) => value,
                (None, Some(server)) if identifier(server) => server,
                _ => return Err(invalid()),
            };
            if item.prefix.len() > 64
                || !item.prefix.bytes().all(|v| (32..=126).contains(&v))
                || item.field.as_ref().is_some_and(|name| !self.secret(name))
            {
                return Err(invalid());
            }
            let name = match (&item.header, &item.env) {
                (Some(name), None) if header(name) => name.to_ascii_lowercase(),
                (None, Some(name))
                    if item.server.is_some() && environment(name) && item.prefix.is_empty() =>
                {
                    name.clone()
                }
                _ => return Err(invalid()),
            };
            if !targets.insert((target, name)) {
                return Err(invalid());
            }
        }
        let mut methods = BTreeSet::new();
        for method in &self.auth {
            if !methods.insert(method.kind()) {
                return Err(invalid());
            }
            match method {
                Auth::Oauth2 {
                    flow,
                    authorize,
                    token,
                    device,
                    client,
                    scopes,
                    ..
                } => {
                    if !self.endpoint(authorize)
                        || !self.endpoint(token)
                        || !self.client(client)
                        || !valid_scopes(scopes)
                        || (*flow == Flow::DeviceCode
                            && device.as_ref().is_none_or(|url| !self.endpoint(url)))
                        || device.as_ref().is_some_and(|url| !self.endpoint(url))
                    {
                        return Err(invalid());
                    }
                }
                Auth::Application {
                    fields,
                    token,
                    registration,
                } => {
                    if !self.client_fields(fields)
                        || !self.endpoint(&token.url)
                        || !pointer(&token.access)
                        || !pointer(&token.expires_in)
                        || token.body.len() > 16
                        || token.body.values().any(|value| {
                            !matches!(value.as_str(), "{client_id}" | "{client_secret}")
                        })
                    {
                        return Err(invalid());
                    }
                    if let Some(registration) = registration {
                        if !self.client_fields(&registration.issues)
                            || !valid_scopes(&registration.scopes)
                            || !registration.flow.valid()
                            || !registration.token.valid()
                        {
                            return Err(invalid());
                        }
                        let text =
                            serde_json::to_value(&registration.flow).map_err(|_| invalid())?;
                        if !self.flow_urls(&text) {
                            return Err(invalid());
                        }
                    }
                }
                Auth::Secret { fields }
                    if fields.is_empty()
                        || fields.len() > 4
                        || fields.iter().any(|name| !self.secret(name))
                        || fields.iter().collect::<BTreeSet<_>>().len() != fields.len() =>
                {
                    return Err(invalid());
                }
                Auth::McpOauth { server, client }
                    if !identifier(server)
                        || client.as_ref().is_some_and(|client| !self.client(client)) =>
                {
                    return Err(invalid());
                }
                Auth::LocalCli {
                    command,
                    env,
                    status,
                    token,
                    timeout_ms,
                    max_output,
                } if command.is_empty()
                    || command.len() > 64
                    || !command.bytes().all(|v| {
                        v.is_ascii_lowercase() || v.is_ascii_digit() || b"._-".contains(&v)
                    })
                    || !command.as_bytes()[0].is_ascii_alphanumeric()
                    || env.len() > 8
                    || env.iter().any(|(name, value)| {
                        !environment(name)
                            || matches!(
                                name.as_str(),
                                "PATH" | "HOME" | "USERPROFILE" | "APPDATA" | "LOCALAPPDATA"
                            )
                            || value.len() > 2048
                            || value.contains('\0')
                    })
                    || !argv(&status.argv, false)
                    || !argv(&token.argv, true)
                    || !pointer(&status.account)
                    || status.condition.as_ref().is_some_and(|condition| {
                        !pointer(&condition.pointer)
                            || condition.values.is_empty()
                            || condition.values.len() > 8
                            || condition.values.iter().any(|value| {
                                !value.is_boolean()
                                    && !value.is_i64()
                                    && !value.as_str().is_some_and(|value| {
                                        value.len() <= 256 && !value.chars().any(char::is_control)
                                    })
                            })
                    })
                    || !(1..=30_000).contains(timeout_ms)
                    || !(1..=65_536).contains(max_output)
                    || matches!(&token.parse, Parse::Json(value) if !pointer(value)) =>
                {
                    return Err(invalid());
                }
                _ => {}
            }
        }
        if self.identity.as_ref().is_some_and(|item| {
            !self.endpoint(&item.request) || !pointer(&item.name) || !pointer(&item.id)
        }) || self.health.as_ref().is_some_and(|item| {
            !self.endpoint(&item.request)
                || item.ok.is_empty()
                || item.ok.len() > 16
                || item.ok.iter().any(|code| !(100..=599).contains(code))
        }) {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn endpoint(&self, value: &str) -> bool {
        let mut expanded = value.to_owned();
        for field in self.fields.iter().filter(|field| !field.secret) {
            expanded = expanded.replace(&format!("{{{}}}", field.name), "tenant");
        }
        !expanded.contains(['{', '}'])
            && url::Url::parse(&expanded).is_ok_and(|url| {
                url.scheme() == "https"
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.fragment().is_none()
                    && self.origins.contains(&url.origin().ascii_serialization())
            })
    }
    fn secret(&self, name: &str) -> bool {
        self.fields
            .iter()
            .any(|field| field.name == name && field.secret)
    }
    fn client_fields(&self, fields: &ClientFields) -> bool {
        self.fields
            .iter()
            .any(|field| field.name == fields.client_id && !field.secret)
            && fields
                .client_secret
                .as_ref()
                .is_none_or(|name| self.secret(name))
    }
    fn client(&self, client: &Client) -> bool {
        match client {
            Client::Bundled { bundled } => {
                !bundled.client_id.is_empty()
                    && bundled.client_id.len() <= 2048
                    && !bundled.client_id.chars().any(char::is_control)
            }
            Client::Fields { fields } => self.client_fields(fields),
        }
    }
    fn flow_urls(&self, value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(map) => map.iter().all(|(key, value)| {
                if matches!(key.as_str(), "url" | "authorize" | "issuer") && !value.is_null() {
                    value.as_str().is_some_and(|url| self.endpoint(url))
                } else {
                    self.flow_urls(value)
                }
            }),
            serde_json::Value::Array(items) => items.iter().all(|value| self.flow_urls(value)),
            _ => true,
        }
    }
}

pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|v| v.is_ascii_lowercase() || v.is_ascii_digit() || b"_-".contains(&v))
}
fn pointer(value: &str) -> bool {
    value.starts_with('/') && value.len() <= 256 && !value.chars().any(char::is_control)
}
fn environment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_uppercase()
        && value
            .bytes()
            .all(|v| v.is_ascii_uppercase() || v.is_ascii_digit() || v == b'_')
}
fn valid_scopes(scopes: &[String]) -> bool {
    scopes.len() <= 64
        && scopes
            .iter()
            .all(|s| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_whitespace))
        && scopes.iter().collect::<BTreeSet<_>>().len() == scopes.len()
}
fn argv(values: &[String], account: bool) -> bool {
    !values.is_empty()
        && values.len() <= 16
        && values.iter().all(|v| {
            let value = if account {
                v.replace("{account}", "account")
            } else {
                v.clone()
            };
            value.len() <= 256 && !value.contains(['\0', '{', '}'])
        })
}
fn header(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|v| v.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&v))
        && !matches!(
            value.to_ascii_lowercase().as_str(),
            "host" | "content-length" | "transfer-encoding" | "connection" | "cookie" | "upgrade"
        )
        && !value.to_ascii_lowercase().starts_with("proxy-")
}
fn origin(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| url.scheme() == "https" && url.username().is_empty() && url.password().is_none() && url.path() == "/" && url.query().is_none() && url.fragment().is_none() && url.origin().ascii_serialization() == value && matches!(url.host(), Some(url::Host::Domain(host)) if host.contains('.') && !["localhost", ".localhost", ".local", ".internal", ".lan", ".home"].iter().any(|suffix| host == *suffix || host.ends_with(suffix))))
}

#[cfg(test)]
mod tests;
