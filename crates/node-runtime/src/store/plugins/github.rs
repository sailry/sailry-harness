//! Explicit CLI linkage targets only the official GitHub MCP endpoint.
use super::*;
use crate::plugins::github::login::Identity;

pub(in crate::store) fn prepare(
    db: &Connection,
    host: &crate::plugins::Host,
    package: &plugin::Reference,
) -> Result<Option<Identity>, Fault> {
    let info = settings::current(db, package)?;
    if package.name != "github" {
        return Err(Fault::invalid("GitHub login requires the GitHub plugin"));
    }
    let binding = info
        .settings
        .as_ref()
        .and_then(|schema| schema.properties.get("token"))
        .and_then(|field| field.secret.as_ref())
        .ok_or_else(|| {
            Fault::new(
                ErrorCode::NotConfigured,
                "GitHub token settings are unavailable",
            )
        })?;
    if binding.unsupported
        || binding.server.as_deref() != Some("github")
        || binding.header.as_deref() != Some("Authorization")
        || binding.env.is_some()
        || binding.origin.is_some()
        || binding.prefix.as_deref() != Some("Bearer ")
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "GitHub token binding changed",
        ));
    }
    let stored = settings::stored(db, &info)?;
    let configuration = match stored.mcp {
        Some(configuration) => configuration,
        None => host.mcp_configuration(package, sailry_link::CancellationToken::new())?,
    };
    if !matches!(
        configuration.servers.get("github"),
        Some(plugin::mcp::Server::Http {url,..}) if url == "https://api.githubcopilot.com/mcp/"
    ) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "GitHub login requires the official MCP endpoint",
        ));
    }
    Ok(stored.github)
}

pub(in crate::store) fn save(
    db: &Connection,
    host: &crate::plugins::Host,
    package: &plugin::Reference,
    identity: Identity,
) -> Result<(Output, Option<Event>), Fault> {
    prepare(db, host, package)?;
    let mut info = required(db, "github")?;
    let mut saved = settings::stored(db, &info)?;
    saved.schema = info.settings.clone();
    saved.slots.remove("token");
    saved.github = Some(identity);
    settings::persist(db, &mut info, &saved)?;
    let state = settings::read(db, &info.summary.reference())?;
    Ok((
        Output::PluginSettings(state),
        Some(Event::PluginChanged(info.summary)),
    ))
}

#[cfg(test)]
mod tests;
