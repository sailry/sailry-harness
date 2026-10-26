//! Persist host and registration identity independently from an unfinished code exchange.
use super::*;
use crate::{
    providers::login::plugin::{Target, workflow::Context},
    store::database::storage_error,
};
use turso::Connection;

pub(super) async fn record(
    database: &mut Database,
    provider: &Provider,
    capture: Option<&crate::providers::login::plugin::Capture>,
    client_id: &str,
) -> Result<(), Fault> {
    use crate::store::database::conflict;
    let write = database.write().await?;
    for attempt in 0..=conflict::RETRIES {
        let recorded = async {
            let transaction = conflict::begin(&database.connection).await?;
            if let Some(capture) = capture {
                super::model_auth::complete(&transaction, provider, capture).await?;
            }
            transaction
                .execute(
                    "INSERT INTO oauth_registrations(provider,client_id) VALUES(?1,?2) ON CONFLICT(provider) DO UPDATE SET client_id=excluded.client_id",
                    turso::params![provider.id.to_string(), client_id],
                )
                .await
                .map_err(storage_error)?;
            conflict::commit(transaction).await
        }
        .await;
        if let Err(error) = conflict::rollback(&database.connection).await {
            database.connection.hold_write(write);
            return Err(error);
        }
        if !conflict::again(&recorded, attempt).await {
            return recorded;
        }
    }
    unreachable!("the final attempt returns its result")
}

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
    async fn records_during_other_writes() {
        let root = tempfile::tempdir().unwrap();
        let mut database = Database::open(&root.path().join("node.turso"), NodeId([87; 32]), None)
            .await
            .unwrap();
        let provider = Provider {
            id: ProviderId::new(),
            revision: 1,
            preset: None,
            binding: None,
            consent: None,
            name: "Registration fixture".into(),
            api: sailry_protocol::conversation::ModelApi::Responses,
            authentication: Authentication::ChatGpt,
            endpoint: crate::providers::login::CHATGPT_ENDPOINT.into(),
            options: None,
            oauth: None,
            enabled: true,
            models: vec![],
            default_model: "fixture".into(),
            request_max_retries: None,
            stream_max_retries: None,
            credential: None,
        };
        let cancel = Request::new(
            database.node,
            Command::CancelProviderLogin {
                attempt: RequestId::new(),
            },
        );
        let other = database.handle.lease(false).await.unwrap();
        // A held writer creates a real storage conflict without relying on
        // commit I/O timing.
        let admission = turso::transaction::Transaction::new_unchecked(
            &other,
            turso::transaction::TransactionBehavior::Immediate,
        )
        .await
        .unwrap();
        admission
            .execute(
                "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')",
                turso::params![
                    &database.node.0[..],
                    cancel.id.to_string(),
                    serde_json::to_vec(&cancel).unwrap(),
                ],
            )
            .await
            .unwrap();
        let mut registering = Box::pin(record(&mut database, &provider, None, "oaiapp_fixture"));
        let initial = futures::poll!(registering.as_mut());
        admission.commit().await.unwrap();
        match initial {
            std::task::Poll::Ready(result) => {
                drop(registering);
                result
            }
            std::task::Poll::Pending => registering.await,
        }
        .expect("registration must survive a concurrent storage write");
        let saved: String = database
            .connection
            .prepare("SELECT client_id FROM oauth_registrations WHERE provider=?1")
            .await
            .unwrap()
            .query_row([provider.id.to_string()])
            .await
            .unwrap()
            .get(0)
            .unwrap();
        assert_eq!(saved, "oaiapp_fixture");
        let status: String = other
            .prepare("SELECT status FROM requests WHERE id=?1")
            .await
            .unwrap()
            .query_row([cancel.id.to_string()])
            .await
            .unwrap()
            .get(0)
            .unwrap();
        assert_eq!(status, "admitted");
        drop(other);
        database.close().await.unwrap();
    }

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
