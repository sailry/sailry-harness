//! Authoritative Node policy; native assertion handles are never stored.
use crate::store::database::{encode, storage_error};
use sailry_protocol::{power::Settings, *};
use turso::Connection;

pub(crate) async fn read(db: &Connection) -> Result<Settings, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM power_settings WHERE singleton=1")
            .await
            .map_err(storage_error)?;
        match statement.query_row(()).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(crate) async fn save(
    db: &Connection,
    settings: &Settings,
) -> Result<(Output, Option<Event>), Fault> {
    if read(db).await?.revision != settings.revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "power settings changed",
        ));
    }
    let mut saved = settings.clone();
    saved.revision = saved
        .revision
        .checked_add(1)
        .ok_or_else(|| Fault::invalid("power settings revision exhausted"))?;
    db.execute("INSERT INTO power_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", [encode(&saved)?]).await.map_err(storage_error)?;
    let event = Event::PowerSettingsChanged {
        revision: saved.revision,
    };
    Ok((Output::PowerSettings(saved), Some(event)))
}

pub(super) async fn failed(
    database: &mut super::database::Database,
    code: i64,
    events: &tokio::sync::broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let publication = database.publication.clone();
    let tx = database
        .connection
        .unchecked_transaction()
        .await
        .map_err(storage_error)?;
    let event = super::notifications::system::power(&tx, code).await?;
    let envelopes = super::database::record(&tx, database.node, vec![event]).await?;
    let publication = publication.lock().await;
    crate::store::database::conflict::commit(tx).await?;
    for envelope in envelopes {
        publication.send(events, envelope);
    }
    Ok(())
}
