//! Application creation binds issued credentials to existing protected settings.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Feishu,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub provider: Provider,
    pub client_id: String,
    pub client_secret: String,
    /// Only these application-identity scopes are requested at provider consent.
    pub tenant_scopes: Vec<String>,
}

impl Declaration {
    pub fn valid(&self) -> bool {
        let field = |value: &str| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        };
        field(&self.client_id)
            && field(&self.client_secret)
            && self.client_id != self.client_secret
            && self.tenant_scopes.len() <= 32
            && self.tenant_scopes.iter().enumerate().all(|(index, scope)| {
                !scope.is_empty()
                    && scope.len() <= 256
                    && scope
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b":._-".contains(&byte))
                    && !self.tenant_scopes[..index].contains(scope)
            })
    }
}
