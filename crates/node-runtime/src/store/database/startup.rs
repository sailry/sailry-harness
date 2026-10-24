//! Schema rehearsal, private backup and recovery precede normal storage admission.
use super::{APPLICATION_ID, connect, driver, engine, record, schema};
use crate::{Error, store};
use sailry_protocol::{NodeId, terminal};
use std::path::Path;
use turso::{Connection, Database};

pub(super) async fn open(
    path: &Path,
    node: NodeId,
    plugins: Option<&crate::plugins::Host>,
) -> Result<(Database, Connection), Error> {
    let mut handle = engine(path).await?;
    let mut connection = connect(&handle).await?;
    let schema = schema::prepare(&connection, path, node, plugins).await?;
    if schema.needs_update() {
        driver::checkpoint(&connection).await?;
        drop(connection);
        drop(handle);
        let preview = schema::backup::preview(path)?;
        {
            let preview_handle = engine(preview.path()).await?;
            let mut preview_connection = connect(&preview_handle).await?;
            let transaction = preview_connection.transaction().await?;
            schema.initialize(&transaction).await?;
            schema.plugins(&transaction).await?;
            transaction.commit().await?;
            driver::checkpoint(&preview_connection).await?;
        }
        schema::backup::save(path)?;
        handle = engine(path).await?;
        connection = connect(&handle).await?;
    }
    let transaction = connection.transaction().await?;
    schema.initialize(&transaction).await?;
    transaction
        .pragma_update("application_id", APPLICATION_ID)
        .await?;
    transaction.pragma_update("user_version", 1).await?;
    let stored = transaction
        .prepare("SELECT identity FROM node WHERE singleton=1")
        .await?
        .query_row(())
        .await;
    match stored {
        Ok(row) if row.get::<Vec<u8>>(0)? == node.0 => {}
        Ok(_) => {
            return Err(Error::Worker(
                "Node identity does not match its database".into(),
            ));
        }
        Err(turso::Error::QueryReturnedNoRows) => {
            transaction
                .execute(
                    "INSERT INTO node(singleton,identity) VALUES(1,?1)",
                    [&node.0[..]],
                )
                .await?;
        }
        Err(error) => return Err(error.into()),
    }
    schema.plugins(&transaction).await?;
    store::plugins::connectors::login::recover(&transaction)
        .await
        .map_err(|error| Error::Worker(error.message))?;
    // An admission with no committed result is uncertain; recovery never replays it.
    transaction
        .execute(
            "UPDATE requests SET status='unknown' WHERE status='admitted'",
            (),
        )
        .await?;
    store::agent::recover(&transaction)
        .await
        .map_err(|error| Error::Worker(error.message))?;
    store::dispatch::jobs::recover(&transaction, node)
        .await
        .map_err(|error| Error::Worker(error.message))?;
    for mut info in store::terminals::list(&transaction)
        .await
        .map_err(|error| Error::Worker(error.message))?
    {
        if info.status != terminal::Status::Running {
            continue;
        }
        info.status = terminal::Status::Stopped;
        info.owner = None;
        info.revision += 1;
        if let Some(event) = store::terminals::record(&transaction, &info)
            .await
            .map_err(|error| Error::Worker(error.message))?
        {
            record(&transaction, node, vec![event])
                .await
                .map_err(|error| Error::Worker(error.message))?;
        }
    }
    transaction.commit().await?;
    Ok((handle, connection))
}
