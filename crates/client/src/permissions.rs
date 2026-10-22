//! One device-scoped permission directory, independent of controller presentation.
use crate::Client;
use sailry_protocol::{
    Command, ErrorCode, Fault, NodeId, Output,
    permissions::{Contribution, Identity, Status},
    plugin::{self, desktop::Surface},
};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub identity: Identity,
    pub package: plugin::Reference,
    pub declaration: Contribution,
}

pub fn directory<'a>(
    device: NodeId,
    packages: impl IntoIterator<Item = &'a plugin::Info>,
) -> Vec<Entry> {
    let mut seen = BTreeSet::new();
    let mut entries = Vec::new();
    for info in packages {
        if !info.summary.enabled {
            continue;
        }
        let Some(extension) = &info.extension else {
            continue;
        };
        if !sailry_protocol::permissions::valid(extension) {
            continue;
        }
        for declaration in &extension.permissions {
            let identity = Identity {
                device,
                subject: declaration.subject.clone(),
                kind: declaration.kind,
            };
            if seen.insert(identity.clone()) {
                entries.push(Entry {
                    identity,
                    package: info.summary.reference(),
                    declaration: declaration.clone(),
                });
            }
        }
    }
    entries
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aggregate {
    Authorized,
    Partial,
    Unauthorized,
}

pub fn aggregate(statuses: impl IntoIterator<Item = bool>) -> Aggregate {
    let mut granted = 0;
    let mut missing = 0;
    for status in statuses {
        if status {
            granted += 1;
        } else {
            missing += 1;
        }
    }
    if missing == 0 && granted > 0 {
        Aggregate::Authorized
    } else if granted > 0 {
        Aggregate::Partial
    } else {
        Aggregate::Unauthorized
    }
}

/// Deduplicate onboarding only within this request's missing set.
pub fn requests<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> Vec<&'a Entry> {
    let mut seen = BTreeSet::new();
    entries
        .into_iter()
        .filter(|entry| {
            entry.declaration.request.is_some()
                && seen.insert((
                    entry.identity.device,
                    entry.identity.subject.clone(),
                    entry
                        .declaration
                        .request_group
                        .clone()
                        .unwrap_or_else(|| format!("{:?}", entry.identity.kind)),
                ))
        })
        .collect()
}

impl Client {
    pub async fn permission_status(&self, entry: &Entry, request: bool) -> Result<Status, Fault> {
        if entry.identity.device != self.target() {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "permission belongs to another device",
            ));
        }
        if request {
            let status = self.permission_call(entry, false).await?;
            if !status.can_request
                || status.grants.iter().any(|grant| {
                    grant.kind == entry.identity.kind && grant.authorized == Some(true)
                })
            {
                return Ok(status);
            }
        }
        self.permission_call(entry, request).await
    }

    async fn permission_call(&self, entry: &Entry, request: bool) -> Result<Status, Fault> {
        let handler = if request {
            &entry.declaration.request
        } else {
            &entry.declaration.check
        };
        let handler = handler
            .as_ref()
            .ok_or_else(|| Fault::invalid("permission requires a native handler"))?;
        let input =
            serde_json::json!({"kind":entry.identity.kind,"subject":entry.identity.subject});
        let command = if request {
            Command::CallPlugin {
                handler: handler.clone(),
                input,
            }
        } else {
            Command::ReadPluginCallback {
                handler: handler.clone(),
                input,
            }
        };
        let mut command = self.prepare(command);
        command.plugin = Some(plugin::Context {
            invocation: None,
            turn: None,
            surface: Surface::Permissions,
            package: entry.package.clone(),
            worktree: None,
            session: None,
        });
        let Output::PluginResult(value) = self.execute(command).await? else {
            return Err(Fault::invalid("permission status expected"));
        };
        let status: Status = serde_json::from_value(value)
            .map_err(|_| Fault::invalid("invalid permission status"))?;
        if !status
            .grants
            .iter()
            .any(|grant| grant.kind == entry.identity.kind)
        {
            return Err(Fault::invalid("permission status omits the requested type"));
        }
        Ok(status)
    }
}

#[cfg(test)]
mod tests;
