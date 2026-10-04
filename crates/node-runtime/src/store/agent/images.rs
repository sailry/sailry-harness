//! Images are derived from canonical history, never another attachment store.
use super::*;
use adk_core::FunctionResponseData;

pub(super) fn project(
    db: &Connection,
    turn: TurnId,
    event: &AdkEvent,
    part: usize,
    response: &FunctionResponseData,
) -> Result<Vec<Image>, Fault> {
    let mut images = Vec::new();
    for (index, data) in response.inline_data.iter().enumerate() {
        if supported(&data.mime_type, &data.data) {
            let worktree = runs::get(db, turn)?.worktree;
            images.push(describe(
                turn,
                worktree,
                &event.id,
                part,
                index,
                &data.mime_type,
                &data.data,
            )?);
        }
    }
    Ok(images)
}

pub(super) fn inline(
    db: &Connection,
    turn: TurnId,
    event: &AdkEvent,
    part: usize,
    mime_type: &str,
    data: &[u8],
) -> Result<Image, Fault> {
    if !supported(mime_type, data) {
        return Err(invalid("invalid image in canonical history"));
    }
    let worktree = runs::get(db, turn)?.worktree;
    describe(turn, worktree, &event.id, part, 0, mime_type, data)
}

pub(super) fn resolve(
    db: &Connection,
    session: SessionId,
    image: &Image,
) -> Result<Vec<u8>, Fault> {
    check_session(db, session)?;
    let row: Option<(String, Vec<u8>)> = db
        .query_row(
            "SELECT e.turn,e.body FROM conversation_events h
             JOIN agent_events e ON e.sequence=h.sequence
             WHERE h.session=?1 AND e.id=?2",
            params![session.to_string(), image.entry],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let (turn, body) = row.ok_or_else(missing)?;
    let turn = turn.parse().map_err(storage_error)?;
    let event: AdkEvent = serde_json::from_slice(&body).map_err(storage_error)?;
    let (mime_type, data) = match event
        .content()
        .and_then(|content| content.parts.get(image.part))
    {
        Some(adk_core::Part::FunctionResponse {
            function_response, ..
        }) => {
            let data = function_response
                .inline_data
                .get(image.index)
                .ok_or_else(missing)?;
            (data.mime_type.as_str(), data.data.as_slice())
        }
        Some(adk_core::Part::InlineData {
            mime_type, data, ..
        }) if image.index == 0 => (mime_type.as_str(), data.as_slice()),
        _ => return Err(missing()),
    };
    if !supported(mime_type, data) {
        return Err(missing());
    }
    let worktree = runs::visible(db, session, turn)?.worktree;
    let current = describe(
        turn,
        worktree,
        &event.id,
        image.part,
        image.index,
        mime_type,
        data,
    )?;
    if current != *image {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "image metadata changed",
        ));
    }
    Ok(data.to_vec())
}

fn supported(mime_type: &str, data: &[u8]) -> bool {
    mime_type.starts_with("image/")
        && mime_type.len() <= 255
        && !data.is_empty()
        && data.len() as u64 <= attachment::MAX_BYTES
}

fn describe(
    turn: TurnId,
    worktree: WorktreeId,
    entry: &str,
    part: usize,
    index: usize,
    mime_type: &str,
    data: &[u8],
) -> Result<Image, Fault> {
    let revision = blake3::hash(data).to_hex().to_string();
    let mut identity = blake3::Hasher::new_derive_key("Sailry canonical image v1");
    identity.update(&encode(&(turn, entry, part, index, mime_type, &revision))?);
    let mut bytes: [u8; 16] = identity.finalize().as_bytes()[..16].try_into().unwrap();
    // UUID v8 contains the content-derived identity, with no random or stored state.
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let id = AttachmentId::try_from(uuid::Uuid::from_bytes(bytes)).map_err(storage_error)?;
    let extension = match mime_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        "image/svg+xml" => "svg",
        _ => "img",
    };
    Ok(Image {
        entry: entry.into(),
        part,
        index,
        attachment: attachment::Attachment {
            id,
            spec: attachment::Spec {
                worktree,
                name: format!("image-{}.{extension}", index + 1),
                media_type: mime_type.to_owned(),
                size: data.len() as u64,
                revision,
            },
        },
    })
}

fn missing() -> Fault {
    Fault::new(
        ErrorCode::NotFound,
        "image is unavailable in this conversation",
    )
}
