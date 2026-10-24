//! The MCP SDK stores rotating opaque grants in the connection owner.
use super::*;
use crate::plugins::authorization::Target;

/// An explicit custom-server enable may restore only its existing valid account.
/// Reads and ordinary package servers never change connection state here.
pub(in crate::store::plugins) async fn enable(
    db: &Connection,
    profile: Option<&std::path::Path>,
    info: &Info,
    server: &str,
) -> Result<bool, Fault> {
    if info.mcp_source.is_none() {
        return Ok(false);
    }
    let mut changed = false;
    for declaration in registry::declarations(info) {
        let linked = declaration
            .inject
            .iter()
            .any(|injection| injection.server.as_deref() == Some(server))
            || declaration
                .auth
                .iter()
                .any(|auth| matches!(auth, Auth::McpOauth { server: name, .. } if name == server));
        if !linked {
            continue;
        }
        for mut record in records(db).await?.into_iter().filter(|record| {
            record.plugin == info.summary.name && record.connector == declaration.id
        }) {
            if record.enabled
                || record.method != Kind::McpOauth
                || authorization(&record) != State::Connected
            {
                continue;
            }
            let current = bound(db, profile, info, &declaration, record.method).await?;
            if !consent::same(&record.plugin, record.bound.as_ref(), current.as_ref()) {
                continue;
            }
            super::enabled(&mut record, true)?;
            next(&mut record)?;
            super::save(db, &record).await?;
            changed = true;
        }
    }
    Ok(changed)
}

pub(in crate::store) async fn resolve(
    db: &Connection,
    profile: Option<&std::path::Path>,
    context: &plugin::Context,
    server: &str,
) -> Result<Vec<(crate::plugins::connectors::Target, Injection)>, Fault> {
    let info = if context.turn.is_some() || context.invocation.is_some() {
        crate::store::agent::plugins::packages(db, std::slice::from_ref(&context.package))
            .await?
            .pop()
            .ok_or_else(unavailable)?
    } else {
        settings::current(db, &context.package).await?
    };
    settings::resolve(db, &info).await?;
    let mut targets = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for declaration in registry::declarations(&info) {
        let injections = declaration
            .inject
            .iter()
            .filter(|item| item.server.as_deref() == Some(server))
            .cloned()
            .collect::<Vec<_>>();
        if injections.is_empty() {
            continue;
        }
        let exists = records(db)
            .await?
            .iter()
            .any(|record| record.plugin == info.summary.name && record.connector == declaration.id);
        if !exists && (info.mcp_source.is_some() || declaration.auth.iter().all(|auth|matches!(auth,Auth::Secret{fields} if fields.iter().all(|name|declaration.fields.iter().any(|field|field.name==*name&&field.optional))))) {continue;}
        let (record, declaration) =
            super::resolve(db, profile, context, &declaration.id, None).await?;
        let mut target = super::target(&record, declaration, info.summary.reference());
        target.manual = info.mcp_source.is_some();
        for injection in injections {
            let name = injection
                .header
                .as_ref()
                .map(|name| name.to_ascii_lowercase())
                .or(injection.env.clone())
                .ok_or_else(unavailable)?;
            if !names.insert(name) {
                return Err(Fault::invalid("MCP connector injection targets overlap"));
            }
            targets.push((target.clone(), injection));
        }
    }
    Ok(targets)
}
fn unavailable() -> Fault {
    Fault::new(
        ErrorCode::NotConfigured,
        "MCP connection is unavailable or changed",
    )
}

async fn record(
    db: &Connection,
    profile: Option<&std::path::Path>,
    target: &Target,
) -> Result<Record, Fault> {
    let live = current_target(db, profile, target.id, target.revision, target.checking).await?;
    if live.package.name != target.plugin
        || live.method != Kind::McpOauth
        || !live
            .declaration
            .auth
            .iter()
            .any(|auth| matches!(auth,Auth::McpOauth{server,..} if *server==target.server))
    {
        return Err(unavailable());
    }
    let record = read(db, target.id).await?;
    let endpoint = record
        .bound
        .as_ref()
        .and_then(|bound| bound.targets.get(&target.server))
        .and_then(|server| match server {
            plugin::mcp::Definition::StreamableHttp { url, .. }
            | plugin::mcp::Definition::Sse { url, .. } => Some(url),
            _ => None,
        })
        .ok_or_else(unavailable)?;
    if *endpoint != target.endpoint {
        return Err(unavailable());
    }
    Ok(record)
}
pub(in crate::store) async fn load(
    db: &Connection,
    profile: Option<&std::path::Path>,
    target: &Target,
) -> Result<Option<(u64, Secret)>, Fault> {
    let record = record(db, profile, target).await?;
    Ok(record
        .grant
        .map(|grant| (record.grant_revision, grant.access)))
}
pub(in crate::store) async fn save(
    db: &Connection,
    profile: Option<&std::path::Path>,
    target: &Target,
    revision: u64,
    credentials: Option<Secret>,
) -> Result<Summary, Fault> {
    let record = record(db, profile, target).await?;
    let issued = credentials.map(|access| crate::providers::login::plugin::Issued {
        access,
        refresh: None,
        expires_at_ms: None,
        account_label: None,
        extra: BTreeMap::new(),
    });
    let record = super::renew(db, record.id, record.auth_revision, revision, issued).await?;
    Ok(required(db, &record.plugin).await?.summary)
}

#[cfg(test)]
mod tests;
