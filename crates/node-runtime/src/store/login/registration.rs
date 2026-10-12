//! Persist host and registration identity independently from an unfinished code exchange.
use super::*;
use crate::{providers::login::Authorization, store::database::storage_error};
use rusqlite::{Connection, OptionalExtension};

pub(super) struct Prepared {
    pub authorization: Authorization,
    pub callback: oneshot::Sender<Secret>,
    pub registered: oneshot::Receiver<(String, oneshot::Sender<Result<(), Fault>>)>,
}

pub(super) fn prepare(
    database: &Database,
    provider: &Provider,
    credential: Option<&Credential>,
    redirect: Option<&str>,
) -> Result<Option<Prepared>, Fault> {
    if provider.authentication == Authentication::ClaudeOAuth {
        let (callback, returned) = oneshot::channel();
        let (issued, registered) = oneshot::channel();
        return Ok(Some(Prepared {
            authorization: Authorization {
                redirect: redirect
                    .unwrap_or("https://platform.claude.com/oauth/code/callback")
                    .into(),
                host_id: String::new(),
                previous: None,
                registration: None,
                issued,
                callback: returned,
            },
            callback,
            registered,
        }));
    }
    if provider.authentication != Authentication::ChatGpt {
        if redirect.is_some() {
            return Err(Fault::invalid(
                "device authorization does not use a callback",
            ));
        }
        return Ok(None);
    }
    let redirect = redirect
        .ok_or_else(|| Fault::invalid("ChatGPT authorization requires a controller callback"))?;
    let previous = provider
        .credential
        .as_ref()
        .filter(|_| !credential.is_some_and(|value| value.revoked))
        .map(|reference| {
            let value =
                database.resolve_credential(reference, provider.id, Authentication::ChatGpt)?;
            Grant::decode(Authentication::ChatGpt, &value)
        })
        .transpose()?;
    let registration = database
        .connection
        .query_row(
            "SELECT client_id FROM oauth_registrations WHERE provider=?1",
            [provider.id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let (callback, returned) = oneshot::channel();
    let (issued, registered) = oneshot::channel();
    Ok(Some(Prepared {
        authorization: Authorization {
            redirect: redirect.into(),
            host_id: host_id(&database.connection)?,
            previous,
            registration,
            issued,
            callback: returned,
        },
        callback,
        registered,
    }))
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
