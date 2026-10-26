//! Original tool results are paged from canonical visible events, never a cache.
use super::*;
use sailry_protocol::tool::{MAX_OUTPUT_PAGE_BYTES, OutputPage, OutputReference};

pub(in crate::store) struct Resolved {
    pub session: SessionId,
    pub turn: TurnId,
    pub value: Value,
}

pub(in crate::store) async fn resolve(
    db: &Connection,
    caller: NodeId,
    turn: TurnId,
    reference: &OutputReference,
) -> Result<Resolved, Fault> {
    if reference.call.is_empty()
        || reference.call.len() > 4096
        || reference.revision.len() != 64
        || !reference
            .revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Fault::invalid("tool output reference is invalid"));
    }
    let run = super::super::calls::active(db, turn).await?;
    super::super::runs::visible(db, run.session, turn).await?;
    let saved: Vec<u8> = db
        .prepare("SELECT caller FROM turns WHERE id=?1")
        .await
        .map_err(storage_error)?
        .query_row([turn.to_string()])
        .await
        .map_err(storage_error)?
        .get(0)
        .map_err(storage_error)?;
    if saved.as_slice() != caller.0.as_slice() {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "tool output read belongs to another invocation",
        ));
    }
    let (turn, value) = super::canonical::result(db, run.session, reference).await?;
    Ok(Resolved {
        session: run.session,
        turn,
        value,
    })
}

pub(in crate::store) fn page(
    resolved: Resolved,
    output: OutputReference,
    offset: usize,
    limit: usize,
) -> Result<OutputPage, Fault> {
    let text = resolved.value.to_string();
    if limit == 0 || offset > text.len() || !text.is_char_boundary(offset) {
        return Err(Fault::invalid("tool output page is invalid"));
    }
    let mut end = offset
        .saturating_add(limit.min(MAX_OUTPUT_PAGE_BYTES))
        .min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    if end == offset && end < text.len() {
        end += text[offset..].chars().next().unwrap().len_utf8();
    }
    Ok(OutputPage {
        turn: resolved.turn,
        output,
        offset,
        total_bytes: text.len(),
        next_offset: (end < text.len()).then_some(end),
        text: text[offset..end].into(),
    })
}

#[cfg(test)]
mod tests;
