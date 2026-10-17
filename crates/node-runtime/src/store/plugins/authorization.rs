//! OAuth changes the same immutable plugin configuration revision as other settings.
use super::*;
use sailry_protocol::{
    Secret,
    plugin::{McpTransport, authorization::Status},
};
use std::collections::BTreeMap;

pub(in crate::store) fn current(
    db: &Connection,
    package: &plugin::Reference,
    server: &str,
) -> Result<Info, Fault> {
    let info = settings::current(db, package)?;
    if !info
        .mcp
        .iter()
        .any(|entry| entry.name == server && entry.transport != McpTransport::Stdio)
    {
        return Err(Fault::invalid(
            "MCP authorization requires a declared HTTP server",
        ));
    }
    Ok(info)
}

pub(in crate::store) fn read(
    db: &Connection,
    package: &plugin::Reference,
    server: &str,
) -> Result<Status, Fault> {
    let info = current(db, package, server)?;
    let stored = settings::stored(db, &info)?;
    let configured = stored
        .authorizations
        .get(server)
        .map(|grant| grant.credentials.is_some())
        .unwrap_or(false);
    Ok(Status {
        package: package.clone(),
        server: server.into(),
        configured,
        can_authorize: false,
    })
}

pub(in crate::store) fn inspect(
    db: &Connection,
    package: &plugin::Reference,
    server: &str,
) -> Result<super::super::mcp::Inspection, Fault> {
    let info = current(db, package, server)?;
    let status = read(db, package, server)?;
    let registered = settings::resolve(db, &info)
        .ok()
        .and_then(|settings| {
            crate::plugins::authorization::Registered::resolve(
                info.settings.as_ref(),
                &settings,
                server,
            )
            .ok()
            .flatten()
        })
        .is_some();
    let settings = configuration(db, &info)?;
    let previous_endpoint = credentials(db, &info, server)?.map(|(endpoint, _)| endpoint);
    Ok(super::super::mcp::Inspection {
        info,
        status,
        settings,
        registered,
        previous_endpoint,
    })
}

pub(in crate::store) fn configuration(
    db: &Connection,
    info: &Info,
) -> Result<crate::plugins::settings::Resolved, Fault> {
    let saved = settings::stored(db, info)?;
    Ok(crate::plugins::settings::Resolved {
        values: BTreeMap::new(),
        secrets: BTreeMap::new(),
        authorizations: BTreeMap::new(),
        mcp: saved.mcp,
        github: None,
    })
}

pub(in crate::store) fn credentials(
    db: &Connection,
    info: &Info,
    server: &str,
) -> Result<Option<(String, Secret)>, Fault> {
    Ok(settings::stored(db, info)?
        .authorizations
        .get(server)
        .and_then(|grant| {
            grant
                .credentials
                .clone()
                .map(|credentials| (grant.endpoint.clone(), credentials))
        }))
}

pub(in crate::store) fn save(
    db: &Connection,
    package: &plugin::Reference,
    server: &str,
    endpoint: &str,
    credentials: Secret,
) -> Result<Info, Fault> {
    let mut info = current(db, package, server)?;
    let mut stored = settings::stored(db, &info)?;
    stored.authorizations.insert(
        server.into(),
        super::grants::Grant {
            id: sailry_protocol::CredentialId::new(),
            endpoint: endpoint.into(),
            revision: 1,
            credentials: Some(credentials),
        },
    );
    settings::persist(db, &mut info, &stored)?;
    Ok(info)
}

pub(in crate::store) fn revoke(
    db: &Connection,
    package: &plugin::Reference,
    server: &str,
) -> Result<Info, Fault> {
    let mut info = current(db, package, server)?;
    let mut stored = settings::stored(db, &info)?;
    super::grants::clear(db, &package.name, server)?;
    stored.authorizations.remove(server);
    settings::persist(db, &mut info, &stored)?;
    Ok(info)
}
