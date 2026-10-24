use super::*;
use sailry_protocol::plugin::Action;
use std::path::Path;

pub(super) async fn package(
    db: &Connection,
    context: &plugin::Context,
) -> Result<plugin::Info, Fault> {
    // Shared admission checks the running turn or callback before these handlers.
    if context.turn.is_some() || context.invocation.is_some() {
        return crate::store::agent::plugins::packages(db, std::slice::from_ref(&context.package))
            .await?
            .pop()
            .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "captured plugin is unavailable"));
    }
    let info = current(db, &context.package.name).await?;
    if info.summary.reference() != context.package {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "plugin package changed",
        ));
    }
    Ok(info)
}

async fn current(db: &Connection, name: &str) -> Result<plugin::Info, Fault> {
    let info = super::super::plugins::get(db, name)
        .await?
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "knowledge owner plugin is unavailable"))?;
    if !info.summary.enabled {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "knowledge owner plugin is disabled",
        ));
    }
    Ok(info)
}

pub(super) async fn owner(
    db: &Connection,
    request: &Request,
    selected: Option<&str>,
) -> Result<(Owner, Capture), Fault> {
    if let Some(context) = &request.plugin {
        if selected.is_some() {
            return Err(denied(
                "plugin collection ownership comes from its captured package",
            ));
        }
        let info = package(db, context).await?;
        Ok((
            Owner::Plugin {
                name: info.summary.name.clone(),
            },
            Capture {
                package: Some(info.summary.reference()),
                worktree: context.worktree,
                session: context.session,
            },
        ))
    } else if let Some(name) = selected {
        let info = current(db, name).await?;
        Ok((
            Owner::Plugin { name: name.into() },
            Capture {
                package: Some(info.summary.reference()),
                worktree: None,
                session: None,
            },
        ))
    } else {
        Ok((
            Owner::Controller,
            Capture {
                package: None,
                worktree: None,
                session: None,
            },
        ))
    }
}

fn declares(info: &plugin::Info, action: Action) -> bool {
    info.extension
        .as_ref()
        .is_some_and(|extension| extension.actions.contains(&action))
}

pub(super) fn readable(stored: &Stored, info: &plugin::Info) -> bool {
    match &stored.collection.definition.source {
        Source::Worktree { .. } => declares(info, Action::ReadFiles),
        Source::Storage { scope, .. } => {
            declares(info, Action::ReadStorage)
                && (*scope != Scope::Conversation || declares(info, Action::ReadConversation))
        }
        Source::Directory { .. } | Source::File { .. } => true,
    }
}

pub(super) async fn validate(
    db: &Connection,
    request: &Request,
    definition: &Definition,
    capture: &mut Capture,
) -> Result<(), Fault> {
    definition.validate()?;
    for pattern in &definition.globs {
        globset::Glob::new(pattern).map_err(|_| Fault::invalid("invalid knowledge source glob"))?;
    }
    let info = (match request.plugin.as_ref() {
        Some(value) => Some(
            async {
                let context = value;
                package(db, context).await
            }
            .await,
        ),
        None => None,
    })
    .transpose()?;
    match &definition.source {
        Source::Worktree { worktree, path } => {
            if let Some(context) = &request.plugin
                && (context.worktree != Some(*worktree)
                    || !declares(info.as_ref().unwrap(), Action::ReadFiles))
            {
                return Err(denied(
                    "knowledge source is outside the captured file access",
                ));
            }
            let root: Option<String> = {
                let mut statement = db
                    .prepare("SELECT path FROM worktrees WHERE id=?1")
                    .await
                    .map_err(storage_error)?;
                match statement.query_row([worktree.to_string()]).await {
                    Ok(row) => {
                        let mapped: turso::Result<_> = row.get(0);
                        Some(mapped.map_err(storage_error)?)
                    }
                    Err(turso::Error::QueryReturnedNoRows) => None,
                    Err(error) => return Err(storage_error(error)),
                }
            };
            let root = root.ok_or_else(|| {
                Fault::new(ErrorCode::NotFound, "knowledge worktree is unavailable")
            })?;
            let parts = crate::files::path::components(path, true)?;
            let directory = crate::files::path::root(Path::new(&root))?;
            if let Some((last, parents)) = parts.split_last() {
                let directory = crate::files::path::descend(directory, parents)?;
                let metadata = directory
                    .symlink_metadata(last)
                    .map_err(crate::files::io_error)?;
                if metadata.is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
                    return Err(Fault::invalid(
                        "knowledge sources require regular files or directories",
                    ));
                }
                if metadata.is_dir() {
                    crate::files::path::descend(directory, &[*last])?;
                } else {
                    crate::files::open_regular(&directory, last)?;
                }
            }
            capture.worktree = Some(*worktree);
            capture.session = None;
        }
        Source::Directory { path } | Source::File { path } => {
            if request.plugin.is_some() {
                return Err(denied(
                    "absolute knowledge sources require a controller selection",
                ));
            }
            let source = crate::knowledge::source::native(path)?;
            if !source.is_absolute()
                || source.components().any(|component| {
                    matches!(
                        component,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                })
            {
                return Err(Fault::invalid("knowledge sources require an absolute path"));
            }
            if matches!(&definition.source, Source::Directory { .. }) {
                crate::files::path::root(&source)?;
            } else {
                let parent = source
                    .parent()
                    .ok_or_else(|| Fault::invalid("knowledge file has no parent"))?;
                let name = source
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| Fault::invalid("invalid knowledge file name"))?;
                let directory = crate::files::path::root(parent)?;
                crate::files::open_regular(&directory, name)?;
            }
            capture.worktree = None;
            capture.session = None;
        }
        Source::Storage { scope, .. } => {
            if capture.package.is_none() {
                return Err(denied(
                    "knowledge storage sources require a real plugin owner",
                ));
            }
            if let Some(info) = &info
                && (!declares(info, Action::ReadStorage)
                    || (*scope == Scope::Conversation && !declares(info, Action::ReadConversation)))
            {
                return Err(denied(
                    "plugin does not declare the captured storage access",
                ));
            }
            if *scope == Scope::Conversation {
                let session = capture
                    .session
                    .ok_or_else(|| denied("conversation storage requires a captured session"))?;
                let worktree: Option<String> = {
                    let mut statement = db
                        .prepare("SELECT worktree FROM sessions WHERE id=?1 AND state!='removed'")
                        .await
                        .map_err(storage_error)?;
                    match statement.query_row([session.to_string()]).await {
                        Ok(row) => {
                            let mapped: turso::Result<_> = row.get(0);
                            Some(mapped.map_err(storage_error)?)
                        }
                        Err(turso::Error::QueryReturnedNoRows) => None,
                        Err(error) => return Err(storage_error(error)),
                    }
                };
                let worktree = worktree
                    .ok_or_else(|| {
                        Fault::new(ErrorCode::NotFound, "knowledge conversation is unavailable")
                    })?
                    .parse::<WorktreeId>()
                    .map_err(storage_error)?;
                if capture.worktree != Some(worktree) {
                    return Err(denied(
                        "conversation storage is outside the captured worktree",
                    ));
                }
            } else {
                capture.session = None;
                capture.worktree = None;
            }
        }
    }
    for name in &definition.shared_with {
        current(db, name).await?;
    }
    Ok(())
}

pub(super) fn check(stored: &Stored) -> Result<(), Fault> {
    let collection = &stored.collection;
    collection.definition.validate()?;
    for pattern in &collection.definition.globs {
        globset::Glob::new(pattern)
            .map_err(|_| Fault::invalid("invalid stored knowledge source glob"))?;
    }
    if collection.revision == 0
        || collection.revision > i64::MAX as u64
        || collection.rebuild > i64::MAX as u64
        || collection.progress.generation == 0
        || collection.progress.generation > i64::MAX as u64
        || collection.progress.completed > collection.progress.sources
    {
        return Err(Fault::invalid("invalid stored knowledge collection"));
    }
    if stored.automatic && !super::semantic::automatic(stored) {
        return Err(Fault::invalid("invalid automatic knowledge collection"));
    }
    match (&collection.owner, &stored.capture.package) {
        (Owner::Controller, None)
            if !matches!(collection.definition.source, Source::Storage { .. }) => {}
        (Owner::Plugin { name }, Some(package)) if name == &package.name => {
            crate::plugins::validate_name(name)?;
            if package.settings_revision > i64::MAX as u64 {
                return Err(Fault::invalid(
                    "invalid captured plugin configuration revision",
                ));
            }
        }
        _ => {
            return Err(Fault::invalid(
                "knowledge owner does not match its source capture",
            ));
        }
    }
    match &collection.definition.source {
        Source::Worktree { worktree, path } => {
            crate::files::path::components(path, true)?;
            if stored.capture.worktree != Some(*worktree) || stored.capture.session.is_some() {
                return Err(Fault::invalid("invalid knowledge worktree capture"));
            }
        }
        Source::Storage {
            scope: Scope::Conversation,
            ..
        } if stored.capture.session.is_some() && stored.capture.worktree.is_some() => {}
        Source::Storage {
            scope: Scope::Node, ..
        } if stored.capture.session.is_none() && stored.capture.worktree.is_none() => {}
        Source::Directory { path } | Source::File { path }
            if stored.capture.session.is_none() && stored.capture.worktree.is_none() =>
        {
            let path = Path::new(path);
            if !path.is_absolute()
                || path
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir))
            {
                return Err(Fault::invalid("invalid stored absolute knowledge source"));
            }
        }
        _ => return Err(Fault::invalid("invalid knowledge source capture")),
    }
    Ok(())
}
