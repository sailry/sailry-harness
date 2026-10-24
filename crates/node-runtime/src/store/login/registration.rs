//! Persist host and registration identity independently from an unfinished code exchange.
use super::*;
use crate::{
    providers::login::plugin::{Target, workflow::Context},
    store::database::storage_error,
};
use turso::Connection;

pub(super) struct Prepared {
    pub context: Context,
    pub registered: oneshot::Receiver<(String, oneshot::Sender<Result<(), Fault>>)>,
}

pub(super) async fn prepare(
    database: &Database,
    provider: &Provider,
    credential: Option<&Credential>,
    target: &Target,
) -> Result<Prepared, Fault> {
    let previous = if let Some(credential) = credential.filter(|value| !value.revoked)
        && provider.authentication != Authentication::Plugin
        && super::model_auth::check_binding(&database.connection, credential, target)
            .await
            .is_ok()
    {
        let (_, secret) = crate::store::providers::authentication::resolve(
            &database.connection,
            credential.id,
            provider.id,
            provider.authentication,
        )
        .await?;
        Some(Grant::decode(provider.authentication, &secret)?)
    } else {
        None
    };
    let registration = {
        let mut statement = database
            .connection
            .prepare("SELECT client_id FROM oauth_registrations WHERE provider=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([provider.id.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let (issued, registered) = oneshot::channel();
    Ok(Prepared {
        context: Context {
            previous,
            host_id: host_id(&database.connection).await?,
            registration,
            issued: Some(issued),
        },
        registered,
    })
}

async fn host_id(database: &Connection) -> Result<String, Fault> {
    if let Some(value) = {
        let mut statement = database
            .prepare("SELECT host_id FROM oauth_hosts WHERE singleton=1")
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
    } {
        return Ok(value);
    }
    let value = format!("urn:uuid:{}", uuid::Uuid::new_v4());
    database
        .execute(
            "INSERT INTO oauth_hosts(singleton,host_id) VALUES(1,?1)",
            [value.as_str()],
        )
        .await
        .map_err(storage_error)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn remains_stable_across_registrations() {
        let root = tempfile::tempdir().unwrap();
        let handle = crate::store::database::engine(&root.path().join("registration.turso"))
            .await
            .unwrap();
        let database = crate::store::database::connect(&handle).await.unwrap();
        database.execute_batch("BEGIN; CREATE TABLE oauth_hosts(singleton INTEGER PRIMARY KEY CHECK(singleton=1),host_id TEXT NOT NULL); CREATE TABLE oauth_registrations(provider TEXT PRIMARY KEY,client_id TEXT NOT NULL); COMMIT").await.unwrap();
        let first = host_id(&database).await.unwrap();
        let id = uuid::Uuid::parse_str(first.strip_prefix("urn:uuid:").unwrap()).unwrap();
        assert_eq!(id.get_version_num(), 4);
        database.execute("INSERT INTO oauth_registrations(provider,client_id) VALUES('fixture','oaiapp_fixture')", ()).await.unwrap();
        assert_eq!(host_id(&database).await.unwrap(), first);
        assert_eq!(
            database
                .prepare("SELECT count(*) FROM oauth_hosts")
                .await
                .unwrap()
                .query_row(())
                .await
                .and_then(|row| row.get::<i64>(0))
                .unwrap(),
            1
        );
    }
}
