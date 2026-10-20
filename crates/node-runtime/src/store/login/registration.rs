//! Persist host and registration identity independently from an unfinished code exchange.
use super::*;
use crate::{
    providers::login::plugin::{Target, workflow::Context},
    store::database::storage_error,
};
use rusqlite::{Connection, OptionalExtension};

pub(super) struct Prepared {
    pub context: Context,
    pub registered: oneshot::Receiver<(String, oneshot::Sender<Result<(), Fault>>)>,
}

pub(super) fn prepare(
    database: &Database,
    provider: &Provider,
    credential: Option<&Credential>,
    target: &Target,
) -> Result<Prepared, Fault> {
    let previous = if let Some(credential) = credential.filter(|value| !value.revoked)
        && provider.authentication != Authentication::Plugin
        && super::model_auth::check_binding(&database.connection, credential, target).is_ok()
    {
        let (_, secret) = crate::store::providers::authentication::resolve(
            &database.connection,
            credential.id,
            provider.id,
            provider.authentication,
        )?;
        Some(Grant::decode(provider.authentication, &secret)?)
    } else {
        None
    };
    let registration = database
        .connection
        .query_row(
            "SELECT client_id FROM oauth_registrations WHERE provider=?1",
            [provider.id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let (issued, registered) = oneshot::channel();
    Ok(Prepared {
        context: Context {
            previous,
            host_id: host_id(&database.connection)?,
            registration,
            issued: Some(issued),
        },
        registered,
    })
}

fn host_id(database: &Connection) -> Result<String, Fault> {
    if let Some(value) = database
        .query_row(
            "SELECT host_id FROM oauth_hosts WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?
    {
        return Ok(value);
    }
    let value = format!("urn:uuid:{}", uuid::Uuid::new_v4());
    database
        .execute(
            "INSERT INTO oauth_hosts(singleton,host_id) VALUES(1,?1)",
            [&value],
        )
        .map_err(storage_error)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remains_stable_across_registrations() {
        let database = Connection::open_in_memory().unwrap();
        database.execute_batch("CREATE TABLE oauth_hosts(singleton INTEGER PRIMARY KEY CHECK(singleton=1),host_id TEXT NOT NULL); CREATE TABLE oauth_registrations(provider TEXT PRIMARY KEY,client_id TEXT NOT NULL)").unwrap();
        let first = host_id(&database).unwrap();
        let id = uuid::Uuid::parse_str(first.strip_prefix("urn:uuid:").unwrap()).unwrap();
        assert_eq!(id.get_version_num(), 4);
        database.execute("INSERT INTO oauth_registrations(provider,client_id) VALUES('fixture','oaiapp_fixture')", []).unwrap();
        assert_eq!(host_id(&database).unwrap(), first);
        assert_eq!(
            database
                .query_row("SELECT count(*) FROM oauth_hosts", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
