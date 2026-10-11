use super::*;
use sailry_protocol::plugin::settings::Schema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Credentials {
    #[serde(flatten)]
    pub state: StoredCredentials,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<Secret>,
}

#[derive(Clone)]
pub(crate) struct Registered {
    pub id: String,
    pub secret: Secret,
}

impl Registered {
    pub(crate) fn resolve(
        schema: Option<&Schema>,
        settings: &crate::plugins::settings::Resolved,
        server: &str,
    ) -> Result<Option<Self>, Fault> {
        let Some(schema) = schema else {
            return Ok(None);
        };
        for (name, field) in &schema.properties {
            let Some(binding) = &field.secret else {
                continue;
            };
            let Some(oauth) = &binding.oauth else {
                continue;
            };
            if binding.server.as_deref() != Some(server) {
                continue;
            }
            let id = settings
                .values
                .get(&oauth.client_id)
                .and_then(serde_json::Value::as_str)
                .filter(|id| {
                    !id.is_empty() && id.len() <= 2048 && !id.chars().any(char::is_control)
                })
                .ok_or_else(|| {
                    Fault::new(
                        ErrorCode::NotConfigured,
                        "MCP OAuth client ID is not configured",
                    )
                })?;
            let secret = settings.secrets.get(name).ok_or_else(|| {
                Fault::new(
                    ErrorCode::NotConfigured,
                    "MCP OAuth client secret is not configured",
                )
            })?;
            return Ok(Some(Self {
                id: id.into(),
                secret: secret.clone(),
            }));
        }
        Ok(None)
    }

    pub(super) fn fingerprint(&self) -> String {
        let mut hash = blake3::Hasher::new();
        hash.update(self.id.as_bytes());
        hash.update(&[0]);
        hash.update(self.secret.expose().as_bytes());
        hash.finalize().to_hex().to_string()
    }
}
