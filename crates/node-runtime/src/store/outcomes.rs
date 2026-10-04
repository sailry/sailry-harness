use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{ErrorCode, Fault, NodeId, Output, RequestId, RequestOutcome};

use super::database::storage_error;

pub(super) fn inspect(
    db: &Connection,
    caller: NodeId,
    id: RequestId,
    digest: &str,
) -> Result<RequestOutcome, Fault> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid request digest",
        ));
    }
    let previous: Option<(Vec<u8>, String, Option<String>)> = db
        .query_row(
            "SELECT body,status,result FROM requests WHERE caller=?1 AND id=?2",
            params![&caller.0[..], id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let Some((body, status, result)) = previous else {
        return Ok(RequestOutcome::NotAdmitted);
    };
    // Content-bearing admissions retain replay identity, not another copy of their input.
    let matches = match body
        .strip_prefix(b"file-v1:")
        .or_else(|| body.strip_prefix(b"command-v1:"))
        .or_else(|| body.strip_prefix(b"credential-v1:"))
    {
        Some(stored) => stored == digest.as_bytes(),
        None => blake3::hash(&body).to_hex().as_str() == digest,
    };
    if !matches {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "request identifier was already used for different content",
        ));
    }
    resolve(&status, result)
}

pub(super) fn resolve(status: &str, result: Option<String>) -> Result<RequestOutcome, Fault> {
    match status {
        "admitted" => Ok(RequestOutcome::Admitted),
        "unknown" => Ok(RequestOutcome::Unknown),
        "completed" => Ok(RequestOutcome::Completed(Box::new(decode(
            &result.ok_or_else(|| storage_error("missing result"))?,
        )?))),
        _ => Err(storage_error("invalid admission status")),
    }
}

/// Decode the existing receipt without changing its status, body, or replay identity.
pub(in crate::store) fn decode(body: &str) -> Result<Result<Output, Fault>, Fault> {
    let result: Result<Output, Fault> = serde_json::from_str(body).map_err(storage_error)?;
    match result {
        Ok(output) if !output.supported() => Ok(Err(Fault::new(
            ErrorCode::Unavailable,
            "stored request result is not supported; the request will not be replayed",
        ))),
        result => Ok(result),
    }
}
