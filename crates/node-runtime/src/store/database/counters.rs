//! Durable sequence watermarks advance with the business transaction that uses them.
use super::storage_error;
use sailry_protocol::Fault;
use turso::Connection;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::store) enum Namespace {
    Events,
    PluginRecords,
    AgentEvents,
    DispatchEvents,
    DispatchJobs,
    Notifications,
    Sessions,
    Turns,
    Checkpoints,
    Artifacts,
    Background,
}

impl Namespace {
    const ALL: [Self; 11] = [
        Self::Events,
        Self::PluginRecords,
        Self::AgentEvents,
        Self::DispatchEvents,
        Self::DispatchJobs,
        Self::Notifications,
        Self::Sessions,
        Self::Turns,
        Self::Checkpoints,
        Self::Artifacts,
        Self::Background,
    ];

    fn name(self) -> &'static str {
        self.owner().0
    }

    fn owner(self) -> (&'static str, &'static str) {
        match self {
            Self::Events => ("events", "cursor"),
            Self::PluginRecords => ("plugin_conversation_records", "sequence"),
            Self::AgentEvents => ("agent_events", "sequence"),
            Self::DispatchEvents => ("dispatch_events", "sequence"),
            Self::DispatchJobs => ("dispatch_jobs", "sequence"),
            Self::Notifications => ("notifications", "sequence"),
            Self::Sessions => ("sessions", "rowid"),
            Self::Turns => ("turns", "rowid"),
            Self::Checkpoints => ("file_checkpoints", "rowid"),
            Self::Artifacts => ("artifacts", "rowid"),
            Self::Background => ("agent_background", "sequence"),
        }
    }
}

pub(in crate::store) async fn next(
    connection: &Connection,
    namespace: Namespace,
) -> Result<u64, Fault> {
    if connection.is_autocommit().map_err(storage_error)? {
        return Err(storage_error("sequence allocation requires a transaction"));
    }
    let mut statement = connection
        .prepare(
            "INSERT INTO counters(namespace,value) VALUES(?1,1) \
             ON CONFLICT(namespace) DO UPDATE SET value=counters.value+1 \
             WHERE counters.value<9223372036854775807 RETURNING value",
        )
        .await
        .map_err(storage_error)?;
    let row = match statement.query_row([namespace.name()]).await {
        Ok(row) => row,
        Err(turso::Error::QueryReturnedNoRows) => {
            return Err(storage_error("sequence watermark is exhausted"));
        }
        Err(error) => return Err(storage_error(error)),
    };
    let value = row.get::<i64>(0).map_err(storage_error)?;
    if value < 1 {
        return Err(storage_error("invalid sequence watermark"));
    }
    Ok(value as u64)
}

/// Validate persisted bounds without deriving or repairing a deleted watermark.
pub(super) async fn validate(connection: &Connection) -> Result<(), crate::Error> {
    let mut rows = connection
        .query("SELECT namespace,value FROM counters", ())
        .await?;
    let mut watermarks = std::collections::BTreeMap::new();
    while let Some(row) = rows.next().await? {
        let name = row.get::<String>(0)?;
        let value = row.get::<i64>(1)?;
        if value < 1
            || !Namespace::ALL
                .iter()
                .any(|namespace| namespace.name() == name)
        {
            return Err(invalid());
        }
        watermarks.insert(name, value);
    }
    drop(rows);
    for namespace in Namespace::ALL {
        let (table, column) = namespace.owner();
        let row = connection
            .prepare(format!("SELECT coalesce(max({column}),0) FROM {table}"))
            .await?
            .query_row(())
            .await?;
        let maximum = row.get::<i64>(0)?;
        if maximum < 0 || watermarks.get(table).copied().unwrap_or(0) < maximum {
            return Err(invalid());
        }
    }
    Ok(())
}

fn invalid() -> crate::Error {
    crate::Error::Worker("invalid stored sequence watermark; existing data was not changed".into())
}

#[cfg(test)]
#[path = "counters/tests.rs"]
mod tests;
