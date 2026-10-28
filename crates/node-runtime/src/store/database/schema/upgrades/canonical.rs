//! A one-time conversion at startup; runtime readers accept only Record.
use crate::{Error, agent::canonical::Record};
use turso::{Connection, params};

mod legacy;

pub(super) async fn needed(db: &Connection) -> Result<bool, Error> {
    if !db
        .prepare("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='agent_events')")
        .await?
        .query_row(())
        .await?
        .get::<bool>(0)?
    {
        return Ok(false);
    }
    Ok(db
        .prepare("SELECT EXISTS(SELECT 1 FROM agent_events WHERE json_type(body,'$.kind') IS NULL)")
        .await?
        .query_row(())
        .await?
        .get(0)?)
}

pub(super) async fn apply(db: &Connection) -> Result<(), Error> {
    let mut after = 0;
    loop {
        let mut query = db
            .prepare(
                "SELECT sequence,turn,body FROM agent_events WHERE sequence>?1
                 AND json_type(body,'$.kind') IS NULL ORDER BY sequence LIMIT 16",
            )
            .await?;
        let mut rows = query.query([after]).await?;
        let mut page = Vec::new();
        while let Some(row) = rows.next().await? {
            page.push((
                row.get::<i64>(0)?,
                row.get::<String>(1)?,
                row.get::<Vec<u8>>(2)?,
            ));
        }
        drop(rows);
        drop(query);
        if page.is_empty() {
            return Ok(());
        }
        for (sequence, turn, body) in page {
            let turn = turn
                .parse()
                .map_err(|_| invalid(sequence, "turn", "invalid identifier"))?;
            let record = legacy::capture(db, turn, &body)
                .await
                .map_err(|error| invalid(sequence, "conversion", &format!("{:?}", error.code)))?;
            crate::agent::canonical::validate(&record)
                .map_err(|error| invalid(sequence, "references", &format!("{:?}", error.code)))?;
            let body = serde_json::to_vec(&record)
                .map_err(|error| json_error(sequence, "encode", error))?;
            // Validate the current format before replacing this row in the same transaction.
            serde_json::from_slice::<Record>(&body)
                .map_err(|error| json_error(sequence, "current format", error))?;
            db.execute(
                "UPDATE agent_events SET body=?2 WHERE sequence=?1",
                params![sequence, body],
            )
            .await?;
            after = sequence;
        }
    }
}

fn json_error(sequence: i64, stage: &str, error: serde_json::Error) -> Error {
    invalid(
        sequence,
        stage,
        &format!(
            "{:?}, line {}, column {}",
            error.classify(),
            error.line(),
            error.column()
        ),
    )
}

fn invalid(sequence: i64, stage: &str, category: &str) -> Error {
    Error::Worker(format!(
        "conversation record {sequence} cannot be upgraded ({stage}: {category}); existing data was not changed"
    ))
}
