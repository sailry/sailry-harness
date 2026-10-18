//! Durable metadata references immutable profile-owned HTML revisions.
use super::{
    commands,
    database::{encode, storage_error},
};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{
    artifact::{self, Revision, Summary},
    *,
};
use std::path::Path;
#[path = "artifact/history.rs"]
mod history;
#[path = "artifact/reads.rs"]
mod reads;
pub(in crate::store) use history::{cleanup, prune};
#[cfg(test)]
#[path = "artifact/tests.rs"]
mod tests;

pub(super) fn execute(
    db: &Connection,
    profile: Option<&Path>,
    request: &Request,
) -> Result<(Output, Option<Event>), Fault> {
    let output = match &request.command {
        Command::WriteArtifact(draft) => write(db, profile, request, draft)?,
        Command::ReadArtifact {
            id,
            revision,
            session,
        } => Output::Artifact(read(
            db,
            profile,
            *id,
            *revision,
            scope(db, request, *id, *session)?,
        )?),
        Command::ListArtifacts {
            session,
            after,
            limit,
        } => Output::Artifacts(list(db, *session, *after, *limit)?),
        Command::ListArtifactRevisions {
            id,
            before,
            limit,
            session,
        } => Output::ArtifactRevisions(revisions(
            db,
            *id,
            *before,
            *limit,
            scope(db, request, *id, *session)?,
        )?),
        _ => return Err(Fault::invalid("artifact command required")),
    };
    Ok((output, None))
}

pub(in crate::store) fn required(db: &Connection, id: ArtifactId) -> Result<Summary, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM artifacts WHERE id=?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let value: Summary = serde_json::from_slice(
        &body.ok_or_else(|| Fault::new(ErrorCode::NotFound, "artifact does not exist"))?,
    )
    .map_err(storage_error)?;
    if !value.valid() {
        return Err(storage_error("invalid artifact metadata"));
    }
    Ok(value)
}

pub(in crate::store) fn scope(
    db: &Connection,
    request: &Request,
    id: ArtifactId,
    session: Option<SessionId>,
) -> Result<SessionId, Fault> {
    let captured = request.plugin.as_ref().and_then(|context| context.session);
    if captured.is_some() && session.is_some() && captured != session {
        return Err(denied("artifact read is outside the captured session"));
    }
    let session = captured.or(session).unwrap_or(required(db, id)?.session);
    commands::session(db, session).map_err(|error| {
        if captured.is_some() {
            denied("artifact read has no live captured session")
        } else {
            error
        }
    })?;
    Ok(session)
}

fn write(
    db: &Connection,
    profile: Option<&Path>,
    request: &Request,
    draft: &artifact::Write,
) -> Result<Output, Fault> {
    let profile = profile
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "artifact storage is unavailable"))?;
    let context = request
        .plugin
        .as_ref()
        .ok_or_else(|| denied("artifact writes require an admitted plugin tool"))?;
    let turn = context
        .turn
        .ok_or_else(|| denied("artifact writes require an admitted turn"))?;
    let session = context
        .session
        .ok_or_else(|| denied("artifact writes require a session"))?;
    if context.invocation.is_some() {
        return Err(denied("callbacks cannot write conversation artifacts"));
    }
    let owner = commands::session(db, session)?;
    if owner.delegation.is_some() {
        return Err(denied("delegated sessions cannot write artifacts"));
    }
    let run = super::agent::visible_run(db, session, turn)?;
    if run.origin.is_some()
        || run.status != conversation::Status::Running
        || Some(run.worktree) != context.worktree
    {
        return Err(Fault::new(
            ErrorCode::Cancelled,
            "artifact turn is no longer active",
        ));
    }
    authorize(db, request, context, draft)?;
    if !artifact::label(&draft.title, false)
        || !artifact::label(&draft.summary, true)
        || draft.id.is_some() != draft.expected_revision.is_some()
    {
        return Err(Fault::invalid(
            "invalid artifact title, summary or expected revision",
        ));
    }
    let previous = draft.id.map(|id| required(db, id)).transpose()?;
    if let Some(previous) = &previous {
        if previous.session != session || previous.package.name != context.package.name {
            return Err(denied("artifact belongs to another session or package"));
        }
        if Some(previous.revision) != draft.expected_revision {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "artifact revision changed; read it before editing",
            ));
        }
    }
    let package = super::agent::plugins::package(db, context)?;
    let allowed_classes = package
        .extension
        .as_ref()
        .and_then(|extension| extension.artifact_view.as_ref())
        .map(|view| view.allowed_classes.as_slice())
        .unwrap_or_default();
    let (html, warnings) = match draft.kind {
        artifact::Kind::Document => crate::artifacts::sanitize(&draft.html, allowed_classes)?,
        artifact::Kind::Preview => crate::artifacts::preview(&draft.html)?,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(storage_error)?
        .as_millis()
        .try_into()
        .map_err(storage_error)?;
    let mut artifact = Summary {
        id: draft.id.unwrap_or_default(),
        kind: draft.kind,
        session,
        turn,
        package: context.package.clone(),
        title: draft.title.clone(),
        summary: draft.summary.clone(),
        revision: previous
            .as_ref()
            .map_or(Some(1), |value| value.revision.checked_add(1))
            .ok_or_else(|| Fault::invalid("artifact revision exhausted"))?,
        size: html.len() as u64,
        created_at: previous.as_ref().map_or(now, |value| value.created_at),
        updated_at: now,
    };
    artifact.updated_at = artifact.updated_at.max(artifact.created_at);
    // A tool publishes this summary both as metadata and as an artifact block.
    let content = tool::Content {
        version: 1,
        blocks: vec![tool::Block::Artifact {
            artifact: artifact.clone(),
        }],
    };
    let output = Output::ArtifactWritten {
        artifact: artifact.clone(),
        warnings: warnings.clone(),
    };
    if encode(&(&output, content))?.len()
        > PAGE_BYTES - artifact::MAX_HTML_BYTES - artifact::MAX_VIEW_BYTES
    {
        return Err(Fault::invalid(
            "artifact metadata exceeds the response budget",
        ));
    }
    let referenced: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM artifact_revisions WHERE artifact=?1 AND revision=?2)",
            params![artifact.id.to_string(), artifact.revision],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if referenced {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "artifact revision already exists",
        ));
    }
    // The serialized Node owner may recover a file published before a rolled-back
    // metadata transaction. Referenced revisions remain immutable.
    let digest =
        crate::files::artifacts::write(profile, session, artifact.id, artifact.revision, &html)?;
    db.execute("INSERT INTO artifacts(id,session,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET body=excluded.body",params![artifact.id.to_string(),session.to_string(),encode(&artifact)?]).map_err(storage_error)?;
    db.execute("INSERT INTO artifact_revisions(artifact,revision,body,digest,warnings) VALUES(?1,?2,?3,?4,?5)",params![artifact.id.to_string(),artifact.revision,encode(&artifact)?,digest,encode(&warnings)?]).map_err(storage_error)?;
    Ok(Output::ArtifactWritten { artifact, warnings })
}

fn authorize(
    db: &Connection,
    request: &Request,
    context: &plugin::Context,
    draft: &artifact::Write,
) -> Result<(), Fault> {
    let row: Option<(Vec<u8>, Vec<u8>)> = db
        .query_row(
            "SELECT body,binding FROM agent_approvals WHERE request=?1 AND turn=?2 AND claimed=1",
            params![
                request.id.to_string(),
                context.turn.expect("checked turn").to_string()
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let (body, binding) =
        row.ok_or_else(|| denied("artifact write has no claimed tool approval"))?;
    let approval: conversation::Approval = serde_json::from_slice(&body).map_err(storage_error)?;
    let binding: Option<super::agent::ApprovalBinding> =
        serde_json::from_slice(&binding).map_err(storage_error)?;
    if approval.state != conversation::ApprovalState::Approved
        || binding.as_ref().is_none_or(|binding| {
            binding.owner != context.package
                || binding.operation != Some(tool::Operation::WriteArtifact)
        })
    {
        return Err(denied("artifact write is outside its captured approval"));
    }
    let body: Vec<u8> = db
        .query_row(
            "SELECT body FROM agent_events WHERE turn=?1 AND id=?2",
            params![approval.turn.to_string(), approval.entry],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let event: adk_core::Event = serde_json::from_slice(&body).map_err(storage_error)?;
    let matches = event
        .content()
        .and_then(|content| content.parts.get(approval.index))
        .is_some_and(|part| {
            if let adk_core::Part::FunctionCall { args, name, .. } = part {
                binding
                    .as_ref()
                    .is_some_and(|binding| binding.tool == *name)
                    && serde_json::from_value::<artifact::Write>(args.clone())
                        .is_ok_and(|approved| approved == *draft)
            } else {
                false
            }
        });
    if !matches {
        return Err(denied(
            "artifact write differs from the canonical tool call",
        ));
    }
    Ok(())
}

fn read(
    db: &Connection,
    profile: Option<&Path>,
    id: ArtifactId,
    revision: Option<u32>,
    session: SessionId,
) -> Result<artifact::Content, Fault> {
    let latest = required(db, id)?;
    let visible = if latest.session == session {
        latest.revision
    } else {
        revisions(db, id, None, 1, session)?
            .items
            .first()
            .map(|value| value.revision)
            .ok_or_else(|| denied("artifact has no publication in this conversation"))?
    };
    let revision = revision.unwrap_or(visible);
    let row: Option<(Vec<u8>, String, Vec<u8>)> = db
        .query_row(
            "SELECT body,digest,warnings FROM artifact_revisions WHERE artifact=?1 AND revision=?2",
            params![id.to_string(), revision],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let (body, digest, warnings) =
        row.ok_or_else(|| Fault::new(ErrorCode::NotFound, "artifact revision does not exist"))?;
    let artifact: Summary = serde_json::from_slice(&body).map_err(storage_error)?;
    if artifact.session != session && !history::published(db, session, &artifact)? {
        return Err(denied(
            "artifact revision is outside the conversation history",
        ));
    }
    let html = crate::files::artifacts::read(
        profile.ok_or_else(|| {
            Fault::new(ErrorCode::NotConfigured, "artifact storage is unavailable")
        })?,
        artifact.session,
        id,
        revision,
        artifact.size,
        &digest,
    )?;
    Ok(artifact::Content {
        artifact,
        latest_revision: visible,
        html,
        warnings: serde_json::from_slice(&warnings).map_err(storage_error)?,
        view: None,
    })
}

pub(in crate::store) fn list(
    db: &Connection,
    session: SessionId,
    after: Option<ArtifactId>,
    limit: u16,
) -> Result<artifact::Page, Fault> {
    commands::session(db, session)?;
    let limit = page_limit(limit)?;
    let boundary = if let Some(id) = after {
        if required(db, id)?.session != session {
            return Err(Fault::invalid("artifact cursor belongs to another session"));
        }
        db.query_row(
            "SELECT rowid FROM artifacts WHERE id=?1",
            [id.to_string()],
            |row| row.get::<_, i64>(0),
        )
        .map_err(storage_error)?
    } else {
        i64::MAX
    };
    let mut query = db
        .prepare(
            "SELECT body FROM artifacts WHERE session=?1 AND rowid<?2 ORDER BY rowid DESC LIMIT ?3",
        )
        .map_err(storage_error)?;
    let rows = query
        .query_map(
            params![session.to_string(), boundary, i64::from(limit) + 1],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .map_err(storage_error)?;
    let mut items = Vec::new();
    let mut more = false;
    let mut bytes = encode(&artifact::Page {
        items: Vec::new(),
        next_after: Some(ArtifactId::new()),
    })?
    .len();
    for row in rows {
        if items.len() == usize::from(limit) {
            more = true;
            break;
        }
        let value: Summary =
            serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error)?;
        let size = encode(&value)?.len() + usize::from(!items.is_empty());
        if bytes.saturating_add(size) > PAGE_BYTES {
            if items.is_empty() {
                return Err(Fault::invalid(
                    "artifact metadata exceeds the response budget",
                ));
            }
            more = true;
            break;
        }
        bytes += size;
        items.push(value);
    }
    Ok(artifact::Page {
        next_after: more.then(|| items.last().expect("nonempty page").id),
        items,
    })
}

fn revisions(
    db: &Connection,
    id: ArtifactId,
    before: Option<u32>,
    limit: u16,
    session: SessionId,
) -> Result<artifact::RevisionPage, Fault> {
    let owner = required(db, id)?.session;
    let limit = page_limit(limit)?;
    let mut query=db.prepare("SELECT body FROM artifact_revisions WHERE artifact=?1 AND revision<?2 ORDER BY revision DESC").map_err(storage_error)?;
    let mut rows = query
        .query(params![
            id.to_string(),
            before.map(i64::from).unwrap_or(i64::MAX)
        ])
        .map_err(storage_error)?;
    let mut items = Vec::new();
    let mut more = false;
    let mut bytes = encode(&artifact::RevisionPage {
        items: Vec::new(),
        next_before: Some(u32::MAX),
    })?
    .len();
    while let Some(row) = rows.next().map_err(storage_error)? {
        let value: Summary =
            serde_json::from_slice(&row.get::<_, Vec<u8>>(0).map_err(storage_error)?)
                .map_err(storage_error)?;
        if owner != session && !history::published(db, session, &value)? {
            continue;
        }
        if items.len() == usize::from(limit) {
            more = true;
            break;
        }
        let value = Revision {
            id: value.id,
            kind: value.kind,
            revision: value.revision,
            turn: value.turn,
            title: value.title,
            summary: value.summary,
            size: value.size,
            created_at: value.updated_at,
        };
        let size = encode(&value)?.len() + usize::from(!items.is_empty());
        if bytes.saturating_add(size) > PAGE_BYTES {
            if items.is_empty() {
                return Err(Fault::invalid(
                    "artifact metadata exceeds the response budget",
                ));
            }
            more = true;
            break;
        }
        bytes += size;
        items.push(value);
    }
    if owner != session && items.is_empty() && before.is_none() {
        return Err(denied("artifact has no publication in this conversation"));
    }
    Ok(artifact::RevisionPage {
        next_before: more.then(|| items.last().expect("nonempty page").revision),
        items,
    })
}

// Conversation snapshots also carry history and lifecycle metadata.
const PAGE_BYTES: usize = MAX_FRAME_BYTES / 2;

fn page_limit(limit: u16) -> Result<u16, Fault> {
    if limit == 0 {
        Err(Fault::invalid("artifact page limit is invalid"))
    } else {
        Ok(limit.min(artifact::MAX_PAGE_LIMIT))
    }
}
fn denied(message: &str) -> Fault {
    Fault::new(ErrorCode::PermissionDenied, message)
}

pub(in crate::store) fn validate_stored(db: &Connection) -> Result<(), Fault> {
    let missing: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM artifacts WHERE json_type(body,'$.kind') IS NULL) OR EXISTS(SELECT 1 FROM artifact_revisions WHERE json_type(body,'$.kind') IS NULL)",
        [], |row| row.get(0),
    ).map_err(storage_error)?;
    if missing {
        return Err(storage_error(
            "stored artifact metadata is missing its kind",
        ));
    }
    let mut query=db.prepare("SELECT a.id,a.session,a.body,r.revision,r.body,r.digest,r.warnings FROM artifacts a JOIN artifact_revisions r ON r.artifact=a.id").map_err(storage_error)?;
    let mut rows = query.query([]).map_err(storage_error)?;
    while let Some(row) = rows.next().map_err(storage_error)? {
        let id: String = row.get(0).map_err(storage_error)?;
        let session: String = row.get(1).map_err(storage_error)?;
        let latest: Summary =
            serde_json::from_slice(&row.get::<_, Vec<u8>>(2).map_err(storage_error)?)
                .map_err(storage_error)?;
        let revision: u32 = row.get(3).map_err(storage_error)?;
        let value: Summary =
            serde_json::from_slice(&row.get::<_, Vec<u8>>(4).map_err(storage_error)?)
                .map_err(storage_error)?;
        let digest: String = row.get(5).map_err(storage_error)?;
        let warnings: Vec<String> =
            serde_json::from_slice(&row.get::<_, Vec<u8>>(6).map_err(storage_error)?)
                .map_err(storage_error)?;
        if !latest.valid()
            || !value.valid()
            || latest.id.to_string() != id
            || value.id.to_string() != id
            || latest.session.to_string() != session
            || value.session != latest.session
            || value.revision != revision
            || value.revision > latest.revision
            || value.created_at != latest.created_at
            || value.package.name != latest.package.name
            || digest.len() != 64
            || !digest.bytes().all(|value| value.is_ascii_hexdigit())
            || warnings.len() > 16
            || warnings.iter().any(|warning| warning.len() > 1024)
        {
            return Err(storage_error("incompatible stored artifact metadata"));
        }
    }
    let missing:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM artifacts a WHERE NOT EXISTS(SELECT 1 FROM artifact_revisions r WHERE r.artifact=a.id AND r.revision=json_extract(a.body,'$.revision')))",[],|row|row.get(0)).map_err(storage_error)?;
    if missing {
        return Err(storage_error("artifact latest revision is missing"));
    }
    history::validate(db)?;
    Ok(())
}
