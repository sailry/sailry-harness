use super::*;
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
    pub(crate) fn connector(
        target: &crate::plugins::connectors::Target,
    ) -> Result<Option<Self>, Fault> {
        use sailry_protocol::plugin::connector::{Auth, Client};
        let Some(Auth::McpOauth {
            client: Some(Client::Fields { fields }),
            ..
        }) = target
            .declaration
            .auth
            .iter()
            .find(|auth| auth.kind() == target.method)
        else {
            return Ok(None);
        };
        let Some(name) = &fields.client_secret else {
            return Ok(None);
        };
        let id = target
            .fields
            .get(&fields.client_id)
            .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "MCP client ID is unavailable"))?;
        let secret = target.secrets.get(name).ok_or_else(|| {
            Fault::new(ErrorCode::NotConfigured, "MCP client secret is unavailable")
        })?;
        Ok(Some(Self {
            id: id.clone(),
            secret: secret.clone(),
        }))
    }

    pub(super) fn fingerprint(&self) -> String {
        let mut hash = blake3::Hasher::new();
        hash.update(self.id.as_bytes());
        hash.update(&[0]);
        hash.update(self.secret.expose().as_bytes());
        hash.finalize().to_hex().to_string()
    }
}
