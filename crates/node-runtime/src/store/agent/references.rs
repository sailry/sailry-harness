use super::*;
use sailry_protocol::conversation::reference::Target;

pub(crate) mod history;

pub(crate) const METADATA: &str = "sailry_references";

pub(super) async fn validate(
    db: &Connection,
    session: &sailry_protocol::Session,
    message: &Input,
    captured: Option<&[sailry_protocol::plugin::Reference]>,
) -> Result<(), Fault> {
    let mut targets = Vec::new();
    let mut role = false;
    for reference in &message.references {
        if reference.label.trim().is_empty()
            || reference.label.len() > 512
            || targets.contains(&&reference.target)
        {
            return Err(sailry_protocol::Fault::invalid(
                "references require distinct targets and bounded labels",
            ));
        }
        targets.push(&reference.target);
        if session.config.resource.is_some()
            && !matches!(
                reference.target,
                Target::Database { .. } | Target::Ssh(_) | Target::Session(_)
            )
        {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "connection conversations only accept connection references",
            ));
        }
        match &reference.target {
            Target::Plugin(package) | Target::Skill { package, .. } => {
                capability(db, session, &reference.target, package, captured).await?;
            }
            Target::Ssh(id) => {
                let available = super::connections::catalog(db, session.id).await?;
                if !available.ssh.iter().any(|profile| profile.id == *id) {
                    return Err(Fault::new(
                        ErrorCode::PermissionDenied,
                        "referenced SSH connection is not available to this conversation",
                    ));
                }
            }

            Target::Database {
                connection,
                database,
                table,
            } => {
                let available = super::connections::catalog(db, session.id).await?;
                if !available
                    .databases
                    .iter()
                    .any(|profile| profile.id == *connection)
                {
                    return Err(Fault::new(
                        ErrorCode::PermissionDenied,
                        "referenced database is not available to this conversation",
                    ));
                }
                if database
                    .as_ref()
                    .is_some_and(|name| name.is_empty() || name.len() > 256)
                    || table.as_ref().is_some_and(|table| {
                        database.is_none()
                            || table.schema.is_empty()
                            || table.schema.len() > 256
                            || table.name.is_empty()
                            || table.name.len() > 256
                    })
                {
                    return Err(sailry_protocol::Fault::invalid(
                        "invalid database reference",
                    ));
                }
            }

            Target::File(path) | Target::Directory(path) => {
                crate::files::path::components(path, false)?;
            }
            Target::Agent(selected) => {
                if let Some(binding) = &session.config.assistant {
                    let references = if let Some(captured) = captured {
                        captured.to_vec()
                    } else {
                        super::plugins::assistant(db, binding).await?
                    };
                    let packages = super::plugins::packages(db, &references).await?;
                    let allowed = packages.iter().any(|package| {
                        package.extension.as_ref().is_some_and(|extension| {
                            extension.tools.iter().any(|tool| {
                                (tool.operation.or_else(|| {
                                    tool.handler.as_ref().and_then(|handler| handler.operation)
                                }) == Some(sailry_protocol::tool::Operation::DelegateAgent))
                                    && crate::plugins::conversation::selects(
                                        Some(binding),
                                        &packages,
                                        package,
                                        &tool.name,
                                    )
                            })
                        })
                    });
                    if !allowed {
                        return Err(Fault::new(
                            ErrorCode::PermissionDenied,
                            "plugin assistant does not allow delegation",
                        ));
                    }
                }
                if role
                    || !session
                        .roles
                        .profiles
                        .iter()
                        .any(|profile| profile.reference() == *selected)
                {
                    return Err(sailry_protocol::Fault::invalid(
                        "select one role from the frozen session roster",
                    ));
                }
                role = true;
            }
            Target::Session(id) => {
                history::visible(db, session.project, *id).await?;
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn project(
    event: &crate::agent::canonical::Record,
) -> Result<Vec<reference::Reference>, Fault> {
    if event.provenance.author != "user" {
        return Ok(Vec::new());
    }
    Ok(event
        .metadata
        .input
        .as_ref()
        .map(|input| input.references.clone())
        .unwrap_or_default())
}

/// Streaming drafts still arrive from the execution engine before a durable record exists.
pub(super) fn partial(event: &AdkEvent) -> Result<Vec<reference::Reference>, Fault> {
    if event.author != "user" {
        return Ok(Vec::new());
    }
    event
        .provider_metadata
        .get(METADATA)
        .map(|value| serde_json::from_str(value).map_err(storage_error))
        .transpose()
        .map(Option::unwrap_or_default)
}

async fn capability(
    db: &Connection,
    session: &sailry_protocol::Session,
    target: &Target,
    name: &str,
    captured: Option<&[sailry_protocol::plugin::Reference]>,
) -> Result<(), Fault> {
    use sailry_protocol::plugin::Scope;
    let references = if let Some(binding) = &session.config.assistant {
        vec![binding.package.clone()]
    } else if let Some(captured) = captured {
        captured.to_vec()
    } else {
        super::plugins::capture(db).await?
    };
    let reference = references
        .iter()
        .find(|reference| reference.name == name)
        .ok_or_else(|| {
            sailry_protocol::Fault::invalid("referenced plugin is disabled or unavailable")
        })?;
    let info = super::plugins::packages(db, std::slice::from_ref(reference))
        .await?
        .remove(0);
    if info
        .extension
        .as_ref()
        .is_some_and(|extension| extension.scope == Scope::Desktop)
    {
        return Err(sailry_protocol::Fault::invalid(
            "desktop-only plugins cannot be referenced by an agent",
        ));
    }
    if let Target::Skill { name, .. } = target
        && !info.skills.iter().any(|skill| skill.name == *name)
    {
        return Err(sailry_protocol::Fault::invalid(
            "referenced skill is unavailable in this plugin version",
        ));
    }
    Ok(())
}
