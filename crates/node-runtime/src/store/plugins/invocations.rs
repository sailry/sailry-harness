//! The original admitted request is the authority for a running callback.
use super::*;
use sailry_protocol::{NodeId, Request};

pub(in crate::store) async fn package(
    db: &Connection,
    caller: NodeId,
    context: &plugin::Context,
) -> Result<Info, Fault> {
    let invocation = context.invocation.ok_or_else(denied)?;
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM requests WHERE caller=?1 AND id=?2 AND status='admitted'")
            .await
            .map_err(storage_error)?;
        match statement
            .query_row(params![&caller.0[..], invocation.to_string()])
            .await
        {
            Ok(row) => {
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error).into()),
        }
    };
    let request: Request =
        serde_json::from_slice(&body.ok_or_else(denied)?).map_err(|_| denied())?;
    let mut expected = context.clone();
    expected.invocation = None;
    if !matches!(request.command, Command::CallPlugin { .. })
        || request.plugin.as_ref() != Some(&expected)
    {
        return Err(denied());
    }
    super::super::agent::plugins::packages(db, std::slice::from_ref(&context.package))
        .await?
        .pop()
        .ok_or_else(denied)
}

fn denied() -> Fault {
    Fault::new(
        ErrorCode::PermissionDenied,
        "plugin callback is not active in this scope",
    )
}
