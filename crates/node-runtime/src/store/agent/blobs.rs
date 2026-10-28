//! Transaction-owned binary assets shared by canonical records and their projections.
use crate::{
    agent::canonical::{
        AssetRef, Attachment, BinaryEncoding, BinarySlot, BlockKind, Kind, Payload, Record,
    },
    store::database::storage_error,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use sailry_protocol::{ErrorCode, Fault};
use serde_json::Value;
use turso::{Connection, params};

#[cfg(test)]
mod tests;

pub(crate) async fn put(db: &Connection, mime: &str, bytes: &[u8]) -> Result<AssetRef, Fault> {
    let digest = blake3::hash(bytes).to_hex().to_string();
    db.execute(
        "INSERT INTO agent_blobs(digest,bytes) VALUES(?1,?2) ON CONFLICT(digest) DO NOTHING",
        params![digest.as_str(), bytes],
    )
    .await
    .map_err(storage_error)?;
    Ok(AssetRef {
        digest,
        mime: mime.into(),
        size: bytes.len() as u64,
        uri: None,
        annotations: None,
    })
}

pub(crate) async fn get(db: &Connection, digest: &str) -> Result<Vec<u8>, Fault> {
    match db
        .prepare("SELECT bytes FROM agent_blobs WHERE digest=?1")
        .await
        .map_err(storage_error)?
        .query_row([digest])
        .await
    {
        Ok(row) => row.get(0).map_err(storage_error),
        Err(turso::Error::QueryReturnedNoRows) => Err(Fault::new(
            ErrorCode::NotFound,
            "conversation asset is unavailable",
        )),
        Err(error) => Err(storage_error(error)),
    }
}

/// Capture only declared binary fields; arbitrary tool JSON remains unchanged.
pub(crate) async fn capture(db: &Connection, mut value: Value) -> Result<Payload, Fault> {
    let mut found = Vec::new();
    binaries(&mut value, "", &mut found);
    let mut assets = Vec::with_capacity(found.len());
    for (pointer, mime, bytes, encoding) in found {
        assets.push(BinarySlot {
            pointer,
            asset: put(db, &mime, &bytes).await?,
            encoding,
        });
    }
    Ok(Payload { value, assets })
}

pub(crate) async fn hydrate(db: &Connection, payload: &Payload) -> Result<Value, Fault> {
    let mut value = payload.value.clone();
    for slot in &payload.assets {
        let bytes = get(db, &slot.asset.digest).await?;
        let target = value
            .pointer_mut(&slot.pointer)
            .ok_or_else(|| Fault::invalid("conversation asset slot is invalid"))?;
        *target = match slot.encoding {
            BinaryEncoding::Bytes => serde_json::to_value(bytes).map_err(storage_error)?,
            BinaryEncoding::Base64 => Value::String(STANDARD.encode(bytes)),
        };
    }
    Ok(value)
}

pub(crate) async fn validate(db: &Connection, record: &Record) -> Result<(), Fault> {
    let message = match &record.kind {
        Kind::Message(message) => message,
        Kind::Compaction(compaction) => &compaction.summary,
        _ => return Ok(()),
    };
    for block in &message.content {
        match &block.kind {
            BlockKind::Image { asset } | BlockKind::Document { asset } => check(db, asset).await?,
            BlockKind::ToolResponse(response) => {
                for attachment in &response.attachments {
                    if let Attachment::Inline { asset } = attachment {
                        check(db, asset).await?;
                    }
                }
                check_payload(db, &response.result).await?;
                let original = hydrate(db, &response.result).await?;
                if blake3::hash(original.to_string().as_bytes())
                    .to_hex()
                    .as_str()
                    != response.revision
                {
                    return Err(Fault::invalid("tool output revision is invalid"));
                }
            }
            BlockKind::ServerCall { value }
            | BlockKind::ServerResponse { value }
            | BlockKind::Resource { value } => {
                check_payload(db, value).await?;
            }
            _ => {}
        }
    }
    Ok(())
}

async fn check_payload(db: &Connection, payload: &Payload) -> Result<(), Fault> {
    for slot in &payload.assets {
        if payload.value.pointer(&slot.pointer) != Some(&Value::Null) {
            return Err(Fault::invalid("conversation asset slot is invalid"));
        }
        check(db, &slot.asset).await?;
    }
    Ok(())
}

async fn check(db: &Connection, asset: &AssetRef) -> Result<(), Fault> {
    let bytes = get(db, &asset.digest).await?;
    if bytes.len() as u64 != asset.size || blake3::hash(&bytes).to_hex().as_str() != asset.digest {
        return Err(Fault::invalid("conversation asset is invalid"));
    }
    Ok(())
}

type Binary = (String, String, Vec<u8>, BinaryEncoding);

fn binaries(value: &mut Value, pointer: &str, found: &mut Vec<Binary>) {
    match value {
        Value::Object(object) => {
            let mime = object
                .get("mime_type")
                .or_else(|| object.get("mimeType"))
                .or_else(|| object.get("mime"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            if let Some(mime) = mime {
                for field in ["data", "blob", "bytes"] {
                    let Some(value) = object.get_mut(field) else {
                        continue;
                    };
                    let decoded = match value {
                        Value::String(text) => STANDARD
                            .decode(text.as_bytes())
                            .ok()
                            .filter(|bytes| STANDARD.encode(bytes) == *text)
                            .map(|bytes| (bytes, BinaryEncoding::Base64)),
                        Value::Array(values) => values
                            .iter()
                            .map(|value| value.as_u64().and_then(|value| u8::try_from(value).ok()))
                            .collect::<Option<Vec<u8>>>()
                            .map(|bytes| (bytes, BinaryEncoding::Bytes)),
                        _ => None,
                    };
                    if let Some((bytes, encoding)) = decoded {
                        found.push((format!("{pointer}/{field}"), mime.clone(), bytes, encoding));
                        *value = Value::Null;
                    }
                }
            }
            for (key, value) in object {
                let key = key.replace('~', "~0").replace('/', "~1");
                binaries(value, &format!("{pointer}/{key}"), found);
            }
        }
        Value::Array(array) => {
            for (index, value) in array.iter_mut().enumerate() {
                binaries(value, &format!("{pointer}/{index}"), found);
            }
        }
        _ => {}
    }
}
