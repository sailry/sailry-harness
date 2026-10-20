//! The MCP SDK stores rotating opaque grants in the connection owner.
use super::*;
use crate::plugins::authorization::Target;

pub(in crate::store) fn resolve(
    db: &Connection,
    profile: Option<&std::path::Path>,
    context: &plugin::Context,
    server: &str,
) -> Result<Vec<(crate::plugins::connectors::Target, Injection)>, Fault> {
    let info = if context.turn.is_some() || context.invocation.is_some() {
        crate::store::agent::plugins::packages(db, std::slice::from_ref(&context.package))?
            .pop()
            .ok_or_else(unavailable)?
    } else {
        settings::current(db, &context.package)?
    };
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
        let exists = records(db)?
            .iter()
            .any(|record| record.plugin == info.summary.name && record.connector == declaration.id);
        if !exists && (info.mcp_source.is_some() || declaration.auth.iter().all(|auth|matches!(auth,Auth::Secret{fields} if fields.iter().all(|name|declaration.fields.iter().any(|field|field.name==*name&&field.optional))))) {continue;}
        let (record, declaration) = super::resolve(db, profile, context, &declaration.id, None)?;
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

fn record(
    db: &Connection,
    profile: Option<&std::path::Path>,
    target: &Target,
) -> Result<Record, Fault> {
    let live = current_target(db, profile, target.id, target.revision)?;
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
    let record = read(db, target.id)?;
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
pub(in crate::store) fn load(
    db: &Connection,
    profile: Option<&std::path::Path>,
    target: &Target,
) -> Result<Option<(u64, Secret)>, Fault> {
    let record = record(db, profile, target)?;
    Ok(record
        .grant
        .map(|grant| (record.grant_revision, grant.access)))
}
pub(in crate::store) fn save(
    db: &Connection,
    profile: Option<&std::path::Path>,
    target: &Target,
    revision: u64,
    credentials: Option<Secret>,
) -> Result<Summary, Fault> {
    let mut record = record(db, profile, target)?;
    super::super::super::commands::check_revision(record.grant_revision, revision)?;
    record.grant_revision = revision
        .checked_add(1)
        .filter(|revision| *revision <= i64::MAX as u64)
        .ok_or_else(|| Fault::new(ErrorCode::Conflict, "MCP grant revision exhausted"))?;
    record.grant = credentials.map(|access| Grant {
        access,
        refresh: None,
        expires_at_ms: None,
        extra: BTreeMap::new(),
    });
    if record.grant.is_none() {
        record.state = State::Expired;
    }
    super::save(db, &record)?;
    Ok(required(db, &record.plugin)?.summary)
}
