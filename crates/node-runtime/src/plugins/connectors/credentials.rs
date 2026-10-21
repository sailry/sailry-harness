use super::*;

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Credential {
    Field(String),
    Grant,
}

impl Target {
    pub(crate) fn credential(&self, injection: &Injection) -> Result<Credential, Fault> {
        if let Some(field) = &injection.field {
            return Ok(Credential::Field(field.clone()));
        }
        match self
            .declaration
            .auth
            .iter()
            .find(|auth| auth.kind() == self.method)
            .ok_or_else(unavailable)?
        {
            Auth::Secret { fields } => fields
                .first()
                .cloned()
                .map(Credential::Field)
                .ok_or_else(unavailable),
            _ => Ok(Credential::Grant),
        }
    }
}

impl Credential {
    pub(crate) fn read(&self, target: &Target) -> Result<Option<Secret>, Fault> {
        match self {
            Self::Field(name) => {
                let field = target
                    .declaration
                    .fields
                    .iter()
                    .find(|field| field.name == *name && field.secret)
                    .ok_or_else(unavailable)?;
                match target.secrets.get(name) {
                    Some(secret) if !secret.expose().is_empty() => Ok(Some(secret.clone())),
                    _ if field.optional => Ok(None),
                    _ => Err(unavailable()),
                }
            }
            Self::Grant => {
                let grant = target.grant.as_ref().ok_or_else(unavailable)?;
                if target.method == Kind::McpOauth {
                    super::super::authorization::access(&grant.access).map(Some)
                } else {
                    Ok(Some(grant.access.clone()))
                }
            }
        }
    }
}

impl Service {
    pub(crate) async fn access_for(
        &self,
        target: &Target,
        credential: &Credential,
        ingress: &crate::store::Ingress,
        rejected: Option<u64>,
        stop: &CancellationToken,
    ) -> Result<Option<Access>, Fault> {
        if *credential == Credential::Grant && target.method != Kind::McpOauth {
            return self.access(target, ingress, rejected, stop).await.map(Some);
        }
        let live = ingress
            .connector_target(target.id, target.revision, target.checking)
            .await?;
        Ok(credential.read(&live)?.map(|token| Access {
            token,
            revision: live.grant_revision,
        }))
    }
}

#[cfg(test)]
mod tests;
