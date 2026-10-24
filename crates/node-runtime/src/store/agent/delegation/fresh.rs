use super::*;

#[cfg(test)]
mod tests;

pub(super) async fn capture(
    db: &Connection,
    node: NodeId,
    run: &Run,
    previous: &Session,
    args: &Arguments,
) -> Result<(Session, Option<role::Profile>), Fault> {
    let worktree = match args.worktree {
        Some(worktree) => {
            super::super::super::worktrees::select(
                db,
                previous.project.ok_or_else(|| {
                    sailry_protocol::Fault::invalid(
                        "connection conversations cannot select a project worktree",
                    )
                })?,
                Some(worktree),
            )
            .await?
        }
        None => run.worktree,
    };
    let roster = super::super::super::sessions::roles::for_turn(db, run.turn).await?;
    let role = match args.role.as_deref() {
        Some(key) => Some(
            roster
                .profiles
                .iter()
                .find(|role| role.key == key)
                .ok_or_else(|| {
                    sailry_protocol::Fault::invalid("role is not in the parent turn snapshot")
                })?
                .clone(),
        ),
        None => None,
    };
    let mut config = super::super::permissions::config(db, run.turn).await?;
    if role.as_ref().is_some_and(|role| role.read_only) {
        config.mode = WorkMode::Plan;
    }
    let references = super::super::plugins::read(db, run.turn).await?;
    let plugins = super::super::plugins::packages(db, &references).await?;
    if let Some(role) = &role {
        let resources: Vec<_> = plugins
            .into_iter()
            .filter(|package| {
                crate::plugins::conversation::owns_resources(config.assistant.as_ref(), package)
            })
            .collect();
        for key in &role.skills {
            crate::plugins::resources::skill(&resources, key)?;
        }
    }
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT provider FROM agent_runs WHERE turn=?1")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([run.turn.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped.map_err(storage_error)?
    };
    let mut provider: Option<Provider> = body
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()?;
    if let Some(model) = role.as_ref().and_then(|role| role.model.as_ref()) {
        let selected = roster
            .providers
            .iter()
            .find(|provider| provider.id == model.provider)
            .ok_or_else(|| sailry_protocol::Fault::invalid("frozen role provider is missing"))?
            .clone();
        let metadata = selected
            .models
            .iter()
            .find(|entry| entry.id == model.model)
            .ok_or_else(|| sailry_protocol::Fault::invalid("frozen role model is missing"))?;
        if config.provider != selected.id || config.model != model.model {
            config.features.clear();
        } else {
            metadata.retain_features(selected.api, &mut config.features);
        }
        config.provider = selected.id;
        config.credential = selected.credential.clone();
        config.model = model.model.clone();
        config.effort = model.effort.unwrap_or(metadata.default_effort);
        provider = Some(selected);
    }
    if let Some(provider) = &mut provider {
        crate::store::providers::presets::capture(db, provider).await?;
        crate::providers::login::capture(provider)?;
    }
    super::super::super::commands::validate_config(db, node, &config).await?;
    Ok((
        Session {
            id: SessionId::new(),
            creation_order: 0,
            archived: false,
            activity: Default::default(),
            project: previous.project,
            worktree,
            revision: 1,
            config,
            roles: role::Snapshot::default(),
            fork: None,
            delegation: None,
            profile: provider.map(|provider| SessionProfile {
                source: node,
                provider,
            }),
        },
        role,
    ))
}
