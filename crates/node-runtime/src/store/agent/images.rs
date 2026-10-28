//! Images are derived from canonical history, never another attachment store.
use super::*;
use crate::agent::canonical::{AssetRef, Attachment, BlockKind, Record, ToolFormat, ToolResponse};

pub(super) async fn project(
    db: &Connection,
    turn: TurnId,
    event: &Record,
    part: usize,
    response: &ToolResponse,
) -> Result<Vec<Image>, Fault> {
    let mut images = Vec::new();
    for (index, asset) in assets(response).enumerate() {
        if supported(&asset.mime, asset.size) {
            let worktree = runs::get(db, turn).await?.worktree;
            images.push(describe(turn, worktree, &event.id, part, index, asset)?);
        }
    }
    Ok(images)
}

pub(super) async fn inline(
    db: &Connection,
    turn: TurnId,
    event: &Record,
    part: usize,
    asset: &AssetRef,
) -> Result<Image, Fault> {
    if !supported(&asset.mime, asset.size) {
        return Err(sailry_protocol::Fault::invalid(
            "invalid image in canonical history",
        ));
    }
    let worktree = runs::get(db, turn).await?.worktree;
    describe(turn, worktree, &event.id, part, 0, asset)
}

pub(super) async fn resolve(
    db: &Connection,
    session: SessionId,
    image: &Image,
) -> Result<Vec<u8>, Fault> {
    check_session(db, session).await?;
    let row: Option<(String, Vec<u8>)> = {
        let mut statement = db
            .prepare(
                "SELECT e.turn,e.body FROM conversation_events h
             JOIN agent_events e ON e.sequence=h.sequence
             WHERE h.session=?1 AND e.id=?2",
            )
            .await
            .map_err(storage_error)?;
        match statement
            .query_row(params![session.to_string(), image.entry.as_str()])
            .await
        {
            Ok(row) => {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let (turn, body) = row.ok_or_else(missing)?;
    let turn = turn.parse().map_err(storage_error)?;
    let event: Record = serde_json::from_slice(&body).map_err(storage_error)?;
    let asset = match event
        .message()
        .and_then(|message| {
            message
                .content
                .iter()
                .find(|block| block.index == image.part)
        })
        .map(|block| &block.kind)
    {
        Some(BlockKind::ToolResponse(response)) => {
            assets(response).nth(image.index).ok_or_else(missing)?
        }
        Some(BlockKind::Image { asset }) if image.index == 0 => asset,
        _ => return Err(missing()),
    };
    if !supported(&asset.mime, asset.size) {
        return Err(missing());
    }
    let data = super::blobs::get(db, &asset.digest).await?;
    if data.len() as u64 != asset.size || blake3::hash(&data).to_hex().as_str() != asset.digest {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "image data changed",
        ));
    }
    let worktree = runs::visible(db, session, turn).await?.worktree;
    let current = describe(turn, worktree, &event.id, image.part, image.index, asset)?;
    if current != *image {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "image metadata changed",
        ));
    }
    Ok(data)
}

fn assets(response: &ToolResponse) -> impl Iterator<Item = &AssetRef> {
    let attachments = response
        .attachments
        .iter()
        .filter_map(|attachment| match attachment {
            Attachment::Inline { asset } => Some(asset),
            Attachment::File { .. } => None,
        });
    let (envelope, prefix) = if response.result.value["kind"] == "computer" {
        (&response.result.value["data"], "/data/inline_data")
    } else {
        (&response.result.value, "/inline_data")
    };
    let declared = (response.format == ToolFormat::Json)
        .then(|| {
            envelope
                .get("inline_data")
                .and_then(serde_json::Value::as_array)
        })
        .flatten()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(move |(index, _)| {
            let pointer = format!("{prefix}/{index}/data");
            response
                .result
                .assets
                .iter()
                .find_map(|slot| (slot.pointer == pointer).then_some(&slot.asset))
        });
    attachments.chain(declared)
}

fn supported(mime_type: &str, size: u64) -> bool {
    mime_type.starts_with("image/")
        && mime_type.len() <= 255
        && size > 0
        && size <= attachment::MAX_BYTES
}

fn describe(
    turn: TurnId,
    worktree: WorktreeId,
    entry: &str,
    part: usize,
    index: usize,
    asset: &AssetRef,
) -> Result<Image, Fault> {
    let revision = asset.digest.clone();
    let mime_type = asset.mime.as_str();
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
                size: asset.size,
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
