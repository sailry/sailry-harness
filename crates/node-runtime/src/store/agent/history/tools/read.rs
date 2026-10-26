use super::*;
use sailry_protocol::tool::detail::{MAX_DETAIL_PAGE_BYTES, Page, Reference};

pub(in crate::store) async fn page(
    db: &Connection,
    reference: Reference,
    cursor: Option<usize>,
    limit: usize,
) -> Result<Page, Fault> {
    if reference.revision.len() != 64
        || !reference
            .revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || reference
            .call
            .as_ref()
            .is_some_and(|call| call.is_empty() || call.len() > 4096)
        || limit == 0
        || limit > MAX_DETAIL_PAGE_BYTES
    {
        return Err(Fault::invalid("tool detail page is invalid"));
    }
    let source =
        super::super::canonical::event(db, reference.session, reference.turn, &reference.entry)
            .await?;
    let raw = raw(
        db,
        source.turn,
        source.sequence,
        &source.event,
        reference.index,
    )
    .await?
    .ok_or_else(|| Fault::new(ErrorCode::NotFound, "tool detail does not exist"))?;
    if raw.id != reference.call {
        return Err(Fault::new(
            ErrorCode::NotFound,
            "tool detail call does not match",
        ));
    }
    if raw.revision() != reference.revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "tool detail changed",
        ));
    }
    slice(reference, raw.detail(), cursor.unwrap_or(0), limit)
}

pub(super) fn slice(
    reference: Reference,
    detail: Detail,
    offset: usize,
    limit: usize,
) -> Result<Page, Fault> {
    let text = serde_json::to_string(&detail).map_err(storage_error)?;
    if limit == 0
        || limit > MAX_DETAIL_PAGE_BYTES
        || offset > text.len()
        || !text.is_char_boundary(offset)
    {
        return Err(Fault::invalid("tool detail cursor is invalid"));
    }
    let mut end = offset.saturating_add(limit).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    if end == offset && end < text.len() {
        return Err(Fault::invalid(
            "tool detail limit splits a Unicode character",
        ));
    }
    Ok(Page {
        reference,
        offset,
        total_bytes: text.len(),
        next_offset: (end < text.len()).then_some(end),
        text: text[offset..end].to_owned(),
    })
}

pub(super) async fn raw(
    db: &Connection,
    turn: TurnId,
    sequence: u64,
    event: &AdkEvent,
    index: usize,
) -> Result<Option<Raw>, Fault> {
    let parts = event
        .content()
        .map(|content| content.parts.as_slice())
        .unwrap_or_default();
    let Some(part) = parts.get(index) else {
        let web: Vec<Raw> = parts
            .iter()
            .filter_map(|part| match part {
                adk_core::Part::ServerToolCall { server_tool_call } => {
                    super::super::web_search::result(
                        server_tool_call,
                        super::super::super::presentation::displays(event)
                            .ok()?
                            .get("web_search"),
                    )
                }
                _ => None,
            })
            .collect();
        let derived_index = index.saturating_sub(parts.len());
        if derived_index < web.len() {
            return Ok(web.into_iter().nth(derived_index));
        }
        let Some(derived_index) =
            derived_index.checked_sub(web.len() + usize::from(event.actions.compaction.is_some()))
        else {
            return Ok(None);
        };
        let mut derived = Vec::new();
        derived.extend(super::super::super::elicitation::project(event)?);
        derived.extend(super::super::super::review::project(event)?);
        return Ok(derived.into_iter().nth(derived_index));
    };
    if let Some(raw) = call(db, turn, event, part).await? {
        return Ok(Some(raw));
    }
    let (id, name, result, images) = match part {
        adk_core::Part::FunctionResponse {
            id,
            function_response,
            ..
        } => (
            id.clone(),
            function_response.name.clone(),
            function_response.response.clone(),
            super::super::super::images::project(db, turn, event, index, function_response).await?,
        ),
        adk_core::Part::ServerToolResponse {
            server_tool_response,
        } => {
            let Some(raw) = super::super::web_search::response(server_tool_response) else {
                return Ok(None);
            };
            (
                raw.id,
                raw.name,
                raw.result.unwrap_or(Value::Null),
                Vec::new(),
            )
        }
        _ => return Ok(None),
    };
    let mut raw = source(db, turn, sequence, &event.branch, id.as_deref(), &name)
        .await?
        .unwrap_or_else(|| Raw::new(id.clone(), name.clone(), Value::Null));
    raw.result = Some(result);
    raw.images = images;
    raw.failure = super::super::super::presentation::failures(event)?
        .into_iter()
        .find(|failure| {
            failure.index == index && Some(&failure.id) == id.as_ref() && failure.name == name
        })
        .map(|failure| failure.failure);
    Ok(Some(raw))
}

async fn call(
    db: &Connection,
    turn: TurnId,
    event: &AdkEvent,
    part: &adk_core::Part,
) -> Result<Option<Raw>, Fault> {
    let displays = super::super::super::presentation::displays(event)?;
    let mut raw = match part {
        adk_core::Part::FunctionCall { id, name, args, .. } => {
            let mut raw = Raw::new(
                id.clone(),
                name.clone(),
                super::super::credentials::project(db, turn, name, args).await?,
            );
            raw.display = displays.get(name).cloned();
            raw
        }
        adk_core::Part::ServerToolCall { server_tool_call } => {
            let Some(raw) =
                super::super::web_search::call(server_tool_call, displays.get("web_search"))
            else {
                return Ok(None);
            };
            raw
        }
        _ => return Ok(None),
    };
    raw.presentation = super::super::super::presentation::read(event)?
        .get(&raw.name)
        .copied()
        .unwrap_or_default();
    raw.grouping = super::super::super::presentation::groupings(event)?
        .get(&raw.name)
        .copied()
        .unwrap_or_default();
    Ok(Some(raw))
}

async fn source(
    db: &Connection,
    turn: TurnId,
    sequence: u64,
    branch: &str,
    id: Option<&str>,
    name: &str,
) -> Result<Option<Raw>, Fault> {
    let Some(id) = id else {
        return Ok(None);
    };
    let mut statement = db
        .prepare(
            "SELECT body FROM agent_events WHERE turn=?1 AND sequence<=?2
         AND json_extract(body,'$.branch')=?3 AND EXISTS(
            SELECT 1 FROM json_each(body,'$.content.parts') part
            WHERE (json_extract(part.value,'$.id')=?4 AND json_extract(part.value,'$.name')=?5)
               OR json_extract(part.value,'$.server_tool_call.id')=?4)
         ORDER BY sequence DESC",
        )
        .await
        .map_err(storage_error)?;
    let mut rows = statement
        .query(params![turn.to_string(), sequence as i64, branch, id, name])
        .await
        .map_err(storage_error)?;
    let mut found = None;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let event: AdkEvent =
            serde_json::from_slice(&row.get::<Vec<u8>>(0).map_err(storage_error)?)
                .map_err(storage_error)?;
        for part in event
            .content()
            .into_iter()
            .flat_map(|content| &content.parts)
        {
            if let Some(raw) = call(db, turn, &event, part).await?
                && raw.id.as_deref() == Some(id)
                && raw.name == name
            {
                if found.is_some() {
                    return Ok(None);
                }
                found = Some(raw);
            }
        }
    }
    Ok(found)
}
