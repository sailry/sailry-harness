use super::*;
use sailry_protocol::tool::detail::{MAX_DETAIL_PAGE_BYTES, Page, Reference};

type SourceKey = (TurnId, String, String, String);

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(in crate::store::agent) struct Sources {
    calls: std::collections::BTreeMap<SourceKey, Vec<Raw>>,
}

impl Sources {
    pub(in crate::store::agent) async fn new(
        db: &Connection,
        events: &[(u64, TurnId, AdkEvent)],
    ) -> Result<Self, Fault> {
        let mut requests = std::collections::BTreeSet::<SourceKey>::new();
        let mut bounds = std::collections::BTreeMap::<TurnId, u64>::new();
        for (sequence, turn, event) in events {
            for part in event
                .content()
                .into_iter()
                .flat_map(|content| &content.parts)
            {
                let call = match part {
                    adk_core::Part::FunctionResponse {
                        id: Some(id),
                        function_response,
                        ..
                    } => Some((id.clone(), function_response.name.clone())),
                    adk_core::Part::ServerToolResponse {
                        server_tool_response,
                    } => super::super::web_search::response(server_tool_response)
                        .and_then(|raw| raw.id.map(|id| (id, raw.name))),
                    _ => None,
                };
                if let Some((id, name)) = call {
                    requests.insert((*turn, event.branch.clone(), id, name));
                    bounds
                        .entry(*turn)
                        .and_modify(|current| *current = (*current).max(*sequence))
                        .or_insert(*sequence);
                }
            }
        }
        let mut sources = Self::default();
        if requests.is_empty() {
            return Ok(sources);
        }
        let bounds: Vec<Value> = bounds
            .into_iter()
            .map(|(turn, sequence)| serde_json::json!({"turn":turn, "sequence":sequence}))
            .collect();
        // Read call events once for this page, including earlier ambiguous IDs.
        let mut statement = db
            .prepare(
                "SELECT e.sequence,e.turn,e.body FROM agent_events e
             JOIN json_each(?1) bound ON e.turn=json_extract(bound.value,'$.turn')
                AND e.sequence<=json_extract(bound.value,'$.sequence')
             WHERE EXISTS(SELECT 1 FROM json_each(e.body,'$.content.parts') part
                WHERE json_type(part.value,'$.args') IS NOT NULL
                   OR json_type(part.value,'$.server_tool_call') IS NOT NULL)
             ORDER BY e.sequence",
            )
            .await
            .map_err(storage_error)?;
        let mut rows = statement
            .query([encode(&bounds)?])
            .await
            .map_err(storage_error)?;
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let sequence = row.get::<i64>(0).map_err(storage_error)? as u64;
            let turn = row
                .get::<String>(1)
                .map_err(storage_error)?
                .parse()
                .map_err(storage_error)?;
            let event: AdkEvent =
                serde_json::from_slice(&row.get::<Vec<u8>>(2).map_err(storage_error)?)
                    .map_err(storage_error)?;
            for part in event
                .content()
                .into_iter()
                .flat_map(|content| &content.parts)
            {
                let identity = match part {
                    adk_core::Part::FunctionCall {
                        id: Some(id), name, ..
                    } => Some((id.as_str(), name.as_str())),
                    adk_core::Part::ServerToolCall { server_tool_call }
                        if server_tool_call["type"] == "web_search_call"
                            || server_tool_call["name"] == "web_search"
                                && server_tool_call.get("input").is_some() =>
                    {
                        server_tool_call["id"].as_str().map(|id| (id, "web_search"))
                    }
                    _ => None,
                };
                let Some((id, name)) = identity else {
                    continue;
                };
                if !requests.contains(&(turn, event.branch.clone(), id.to_owned(), name.to_owned()))
                {
                    continue;
                }
                if let Some(mut raw) = call(db, turn, &event, part).await?
                    && let Some(id) = raw.id.clone()
                {
                    raw.source = Some(sequence);
                    sources
                        .calls
                        .entry((turn, event.branch.clone(), id, raw.name.clone()))
                        .or_default()
                        .push(raw);
                }
            }
        }
        Ok(sources)
    }

    fn source(
        &self,
        turn: TurnId,
        sequence: u64,
        branch: &str,
        id: Option<&str>,
        name: &str,
    ) -> Option<Raw> {
        let mut calls = self
            .calls
            .get(&(turn, branch.to_owned(), id?.to_owned(), name.to_owned()))?
            .iter()
            .filter(|raw| raw.source.is_some_and(|source| source <= sequence));
        let source = calls.next()?;
        calls.next().is_none().then(|| source.clone())
    }
}

pub(in crate::store) async fn page(
    db: &Connection,
    cache: &Cache,
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
    let stamp =
        super::super::canonical::sequence(db, reference.session, reference.turn, &reference.entry)
            .await?;
    if let Some((text, dependency)) = cache.get(&reference, stamp) {
        let valid = if let Some(sequence) = dependency {
            db.prepare("SELECT EXISTS(SELECT 1 FROM agent_events WHERE sequence=?1)")
                .await
                .map_err(storage_error)?
                .query_row([sequence as i64])
                .await
                .map_err(storage_error)?
                .get::<bool>(0)
                .map_err(storage_error)?
        } else {
            true
        };
        if valid {
            let page = slice(reference.clone(), &text, cursor.unwrap_or(0), limit)?;
            cache.complete(&reference, page.next_offset.is_some());
            return Ok(page);
        }
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
    let text: std::sync::Arc<str> = serde_json::to_string(&raw.detail())
        .map_err(storage_error)?
        .into();
    let page = slice(reference.clone(), &text, cursor.unwrap_or(0), limit)?;
    cache.insert(reference.clone(), stamp, text, raw.source)?;
    cache.complete(&reference, page.next_offset.is_some());
    Ok(page)
}

pub(super) fn slice(
    reference: Reference,
    text: &str,
    offset: usize,
    limit: usize,
) -> Result<Page, Fault> {
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

pub(in crate::store::agent) async fn raw(
    db: &Connection,
    turn: TurnId,
    sequence: u64,
    event: &AdkEvent,
    index: usize,
) -> Result<Option<Raw>, Fault> {
    with_sources(db, turn, sequence, event, index, None).await
}

pub(in crate::store::agent) async fn with_sources(
    db: &Connection,
    turn: TurnId,
    sequence: u64,
    event: &AdkEvent,
    index: usize,
    sources: Option<&Sources>,
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
    let mut raw = match sources {
        Some(sources) => sources.source(turn, sequence, &event.branch, id.as_deref(), &name),
        None => source(db, turn, sequence, &event.branch, id.as_deref(), &name).await?,
    }
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
            "SELECT sequence,body FROM agent_events WHERE turn=?1 AND sequence<=?2
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
        let body = row.get::<Vec<u8>>(1).map_err(storage_error)?;
        let event: AdkEvent = serde_json::from_slice(&body).map_err(storage_error)?;
        for part in event
            .content()
            .into_iter()
            .flat_map(|content| &content.parts)
        {
            if let Some(mut raw) = call(db, turn, &event, part).await?
                && raw.id.as_deref() == Some(id)
                && raw.name == name
            {
                if found.is_some() {
                    return Ok(None);
                }
                raw.source = Some(row.get::<i64>(0).map_err(storage_error)? as u64);
                found = Some(raw);
            }
        }
    }
    Ok(found)
}
