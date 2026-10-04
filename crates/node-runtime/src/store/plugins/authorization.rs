//! OAuth changes the same immutable plugin configuration revision as other settings.
use super::*;
use sailry_protocol::{
    Secret,
    plugin::{McpTransport, authorization::Status},
};

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
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
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
    })
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
