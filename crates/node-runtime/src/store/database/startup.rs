//! Schema rehearsal, private backup and recovery precede normal storage admission.
use super::{APPLICATION_ID, connect, driver, engine, record, schema};
use crate::{Error, store};
use sailry_protocol::{NodeId, terminal};
use std::path::{Path, PathBuf};
use turso::{Connection, Database};

#[cfg(test)]
mod tests;

pub(super) enum Source {
    Profile,
    Backup,
}

pub(super) async fn upgrade(
    path: &Path,
    node: NodeId,
    plugins: Option<&crate::plugins::Host>,
    source: Source,
) -> Result<(Database, Connection, Option<PathBuf>), Error> {
    let mut handle = engine(path).await?;
    let mut connection = connect(&handle).await?;
    let schema = schema::prepare(&connection, path, node, plugins).await?;
    if schema.needs_rebuild() {
        if matches!(source, Source::Backup) {
            return Err(sailry_protocol::Fault::invalid(
                "backup storage generation is incompatible",
            )
            .into());
        }
        driver::checkpoint(&connection).await?;
        drop(connection);
        drop(handle);
        return Box::pin(rebuild(path, node, plugins)).await;
    }
    let mut backup = None;
    if schema.needs_update() {
        driver::checkpoint(&connection).await?;
        drop(connection);
        drop(handle);
        let preview = schema::backup::preview(path)?;
        {
            let resources = rehearsal(plugins, node)?;
            let preview_handle = engine(preview.path()).await?;
            let mut preview_connection = connect(&preview_handle).await?;
            initialize(
                &schema,
                &mut preview_connection,
                node,
                resources.as_ref().map(|(_, host)| host),
            )
            .await?;
            driver::checkpoint(&preview_connection).await?;
        }
        backup = Some(schema::backup::save(path)?);
        handle = engine(path).await?;
        connection = connect(&handle).await?;
    }
    initialize(&schema, &mut connection, node, plugins).await?;
    Ok((handle, connection, backup))
}

async fn initialize(
    schema: &schema::Schema,
    connection: &mut Connection,
    node: NodeId,
    plugins: Option<&crate::plugins::Host>,
) -> Result<(), Error> {
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
                    "INSERT INTO node(singleton,identity,tool_defaults) VALUES(1,?1,1)",
                    [&node.0[..]],
                )
                .await?;
        }
        Err(error) => return Err(error.into()),
    }
    schema.plugins(&transaction, plugins).await?;
    transaction.commit().await?;
    Ok(())
}

async fn empty(
    path: &Path,
    node: NodeId,
    plugins: Option<&crate::plugins::Host>,
) -> Result<(Database, Connection), Error> {
    let handle = engine(path).await?;
    let mut connection = connect(&handle).await?;
    let schema = schema::prepare(&connection, path, node, plugins).await?;
    initialize(&schema, &mut connection, node, plugins).await?;
    Ok((handle, connection))
}

async fn rebuild(
    path: &Path,
    node: NodeId,
    plugins: Option<&crate::plugins::Host>,
) -> Result<(Database, Connection, Option<PathBuf>), Error> {
    schema::backup::clear(&path.with_file_name("knowledge.turso"))?;
    schema::backup::clear(path)?;
    let (handle, connection) = Box::pin(empty(path, node, plugins)).await?;
    crate::logging::database_rebuild(node);
    Ok((handle, connection, None))
}

pub(super) async fn open(
    path: &Path,
    node: NodeId,
    plugins: Option<&crate::plugins::Host>,
) -> Result<(Database, Connection), Error> {
    let (handle, mut connection, _) = upgrade(path, node, plugins, Source::Profile).await?;
    let transaction = connection.transaction().await?;
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

fn rehearsal(
    plugins: Option<&crate::plugins::Host>,
    node: NodeId,
) -> Result<Option<(crate::backup::archive::Snapshot, crate::plugins::Host)>, Error> {
    let Some(host) = plugins else {
        return Ok(None);
    };
    let profile = host
        .profile()
        .ok_or_else(|| Error::Worker("plugin storage is unavailable".into()))?;
    let mut snapshot = crate::backup::archive::Snapshot::new(profile, node)
        .map_err(|error| Error::Worker(error.message))?;
    if profile.join("plugins/packages").try_exists()? {
        snapshot
            .capture(
                &["plugins/packages".into()],
                &sailry_link::CancellationToken::new(),
            )
            .map_err(|error| Error::Worker(error.message))?;
    }
    let host = crate::plugins::Host::new(Some(snapshot.directory.path().to_owned()));
    Ok(Some((snapshot, host)))
}
