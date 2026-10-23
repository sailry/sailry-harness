//! A transient directory projection; accounts and configuration stay Node-owned.
use super::*;
use sailry_protocol::plugin::{Info, Mcp, Reference};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Member {
    Account(connector::Id),
    Mcp,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Key {
    pub package: String,
    pub member: Member,
}

#[derive(Clone)]
pub(super) struct Service {
    pub key: Key,
    pub package: Reference,
    pub kind: Option<Type>,
    pub account: Option<Connection>,
    pub info: Option<Info>,
    pub servers: Vec<Mcp>,
}

impl Service {
    pub fn label(&self) -> String {
        if let Some(info) = &self.info
            && info.mcp_source.is_some()
        {
            return crate::plugins::metadata::title(info);
        }
        self.kind
            .as_ref()
            .map(|kind| title(&kind.declaration.display))
            .or_else(|| self.info.as_ref().map(crate::plugins::metadata::title))
            .unwrap_or_else(|| {
                self.account
                    .as_ref()
                    .map(|account| account.connector.clone())
                    .unwrap_or_else(|| self.package.name.clone())
            })
    }

    pub fn manual(&self) -> bool {
        self.key.member == Member::Mcp
    }

    pub fn allowed(&self, server: &Mcp) -> bool {
        // Only a canonically connected, disabled dedicated account is the old
        // second switch. Missing, expired and pending authorization retain the
        // server policy and expose their canonical reason separately.
        server.allowed
            && self.account.as_ref().is_none_or(|account| {
                account.enabled || account.state != connector::State::Connected
            })
    }

    pub fn enabled(&self) -> bool {
        self.kind.as_ref().is_none_or(|kind| kind.enabled)
            && self.info.as_ref().is_none_or(|info| info.summary.enabled)
    }

    pub fn matches(&self, method: filter::Method, query: &str) -> bool {
        let method_matches = match &self.account {
            Some(account) => method.matches(account.method),
            None => self
                .kind
                .as_ref()
                .map_or(method == filter::Method::All, |kind| {
                    kind.declaration
                        .auth
                        .iter()
                        .any(|auth| method.matches(auth.kind()))
                }),
        };
        method_matches
            && format!(
                "{} {} {} {}",
                self.label(),
                self.package.name,
                self.account
                    .as_ref()
                    .and_then(|account| account.identity.as_ref())
                    .map(|identity| identity.name.as_str())
                    .unwrap_or_default(),
                self.servers
                    .iter()
                    .map(|server| server.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .to_lowercase()
            .contains(query)
    }
}

pub(super) fn changed_accounts(
    previous: &[Connection],
    current: &[Connection],
) -> BTreeSet<String> {
    previous
        .iter()
        .filter(|account| current.iter().find(|other| other.id == account.id) != Some(*account))
        .chain(current.iter().filter(|account| {
            previous.iter().find(|other| other.id == account.id) != Some(*account)
        }))
        .map(|account| account.package.name.clone())
        .collect()
}

pub(super) fn collect(
    types: &[Type],
    accounts: &[Connection],
    metadata: &BTreeMap<String, Info>,
) -> Vec<Service> {
    let mut services = Vec::new();
    for info in metadata.values().filter(|info| info.mcp_source.is_some()) {
        let package = info.summary.reference();
        let kind = types.iter().find(|kind| {
            kind.package == package
                && info.mcp.iter().any(|server| {
                    crate::plugins::metadata::binds_server(&kind.declaration, &server.name)
                })
        });
        let account = accounts.iter().find(|account| {
            account.package.name == package.name
                && kind.is_none_or(|kind| account.connector == kind.declaration.id)
        });
        services.push(Service {
            key: Key {
                package: package.name.clone(),
                member: Member::Mcp,
            },
            package,
            kind: kind.cloned(),
            account: account.cloned(),
            info: Some(info.clone()),
            servers: info.mcp.clone(),
        });
    }
    for account in accounts {
        // A custom MCP owns its dedicated authorization. Ordinary MCP
        // declarations remain plugin capabilities, not connection instances.
        if metadata
            .get(&account.package.name)
            .is_some_and(|info| info.mcp_source.is_some())
        {
            continue;
        }
        let kind = types.iter().find(|kind| {
            kind.package.name == account.package.name && kind.declaration.id == account.connector
        });
        let package = kind
            .map(|kind| kind.package.clone())
            .unwrap_or_else(|| account.package.clone());
        let info = metadata
            .get(&package.name)
            .filter(|info| info.summary.reference() == package);
        services.push(Service {
            key: Key {
                package: package.name.clone(),
                member: Member::Account(account.id),
            },
            package,
            kind: kind.cloned(),
            account: Some(account.clone()),
            info: info.cloned(),
            servers: info
                .into_iter()
                .flat_map(|info| &info.mcp)
                .filter(|server| {
                    kind.is_some_and(|kind| {
                        crate::plugins::metadata::binds_server(&kind.declaration, &server.name)
                    })
                })
                .cloned()
                .collect(),
        });
    }
    services.sort_by_cached_key(|service| service.label().to_lowercase());
    services
}

#[cfg(test)]
mod tests;
