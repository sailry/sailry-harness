//! A transient directory projection; accounts and configuration stay Node-owned.
use super::*;
use sailry_protocol::plugin::{Info, Mcp, Reference};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Member {
    Account(String),
    Server(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
            .or_else(|| {
                self.info.as_ref().map(|info| {
                    let label = crate::plugins::metadata::title(info);
                    match &self.key.member {
                        Member::Server(server) if info.mcp.len() > 1 => {
                            format!("{label} · {server}")
                        }
                        _ => label,
                    }
                })
            })
            .unwrap_or_else(|| match &self.key.member {
                Member::Account(id) | Member::Server(id) => id.clone(),
            })
    }

    pub fn enabled(&self) -> bool {
        self.kind.as_ref().is_none_or(|kind| kind.enabled)
            && self.info.as_ref().is_none_or(|info| info.summary.enabled)
    }

    pub fn tools(&self) -> impl Iterator<Item = &sailry_protocol::tool::Declaration> {
        self.info
            .iter()
            .flat_map(|info| &info.extension)
            .flat_map(|extension| &extension.tools)
            .filter(|tool| tool.server.is_none())
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

pub(super) fn collect(
    types: &[Type],
    accounts: &[Connection],
    metadata: &BTreeMap<String, Info>,
) -> Vec<Service> {
    let mut services = Vec::new();
    for kind in types {
        let info = metadata
            .get(&kind.package.name)
            .filter(|info| info.summary.reference() == kind.package);
        let account = accounts.iter().find(|account| {
            account.package == kind.package && account.connector == kind.declaration.id
        });
        services.push(Service {
            key: Key {
                package: kind.package.name.clone(),
                member: Member::Account(kind.declaration.id.clone()),
            },
            package: kind.package.clone(),
            kind: Some(kind.clone()),
            account: account.cloned(),
            info: info.cloned(),
            servers: info
                .into_iter()
                .flat_map(|info| &info.mcp)
                .filter(|server| {
                    crate::plugins::metadata::binds_server(&kind.declaration, &server.name)
                })
                .cloned()
                .collect(),
        });
    }
    for info in metadata.values() {
        let package = info.summary.reference();
        if types
            .iter()
            .any(|kind| kind.package.name == package.name && kind.package != package)
        {
            continue;
        }
        for server in &info.mcp {
            if types.iter().any(|kind| {
                kind.package == package
                    && crate::plugins::metadata::binds_server(&kind.declaration, &server.name)
            }) {
                continue;
            }
            services.push(Service {
                key: Key {
                    package: package.name.clone(),
                    member: Member::Server(server.name.clone()),
                },
                package: package.clone(),
                kind: None,
                account: None,
                info: Some(info.clone()),
                servers: vec![server.clone()],
            });
        }
    }
    // Removed declarations do not erase retained accounts or their management actions.
    for account in accounts {
        if services.iter().any(|service| {
            service
                .account
                .as_ref()
                .is_some_and(|current| current.id == account.id)
        }) {
            continue;
        }
        services.push(Service {
            key: Key {
                package: account.package.name.clone(),
                member: Member::Account(account.connector.clone()),
            },
            package: account.package.clone(),
            kind: None,
            account: Some(account.clone()),
            info: metadata
                .get(&account.package.name)
                .filter(|info| info.summary.reference() == account.package)
                .cloned(),
            servers: Vec::new(),
        });
    }
    services.sort_by_cached_key(|service| service.label().to_lowercase());
    services
}

#[cfg(test)]
mod tests;
