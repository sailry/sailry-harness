//! Read-only provider selection for explicit real-model acceptance.
use sailry_protocol::{Secret, conversation::Provider};

/// Seed only one selected provider into an already initialized, stopped test profile.
pub(crate) async fn seed(
    model: &str,
    id: sailry_protocol::ProviderId,
    profile: &std::path::Path,
    node: sailry_protocol::NodeId,
) -> Provider {
    let (mut provider, mut stored) = selected(model, id).await;
    if let Some(credential) = &mut provider.credential {
        credential.node = node;
    }
    stored["provider"] = serde_json::to_value(&provider).unwrap();
    stored["quota"] = serde_json::Value::Null;
    let (database, mut connection) =
        crate::database_fixture::open(&profile.join("storage/node.turso"), false).await;
    let transaction = connection.transaction().await.unwrap();
    transaction
        .execute(
            "INSERT INTO providers(id,body) VALUES(?1,?2)",
            turso::params![id.to_string(), serde_json::to_vec(&stored).unwrap()],
        )
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    crate::database_fixture::close(database, connection).await;
    provider
}

async fn selected(model: &str, id: sailry_protocol::ProviderId) -> (Provider, serde_json::Value) {
    let profile = std::env::var_os("SAILRY_ACCEPTANCE_PROFILE").expect("source profile required");
    let path = std::fs::canonicalize(std::path::Path::new(&profile).join("storage/node.turso"))
        .expect("source profile database required");
    let (database, connection) = crate::database_fixture::open(&path, true).await;
    let selected = connection
        .prepare("SELECT body FROM providers WHERE id=?1")
        .await
        .unwrap()
        .query_row([id.to_string()])
        .await;
    let body = match selected {
        Ok(row) => row,
        Err(_) => {
            let mut rows = connection
                .query("SELECT body FROM providers", ())
                .await
                .unwrap();
            let mut available = Vec::new();
            while let Some(row) = rows.next().await.unwrap() {
                if let Some((provider, _)) = read(&row.get::<Vec<u8>>(0).unwrap()) {
                    available.push(serde_json::json!({
                        "id":provider.id,"name":provider.name,"api":provider.api,
                        "enabled":provider.enabled,"authentication":provider.authentication,
                        "models":provider.models.iter().map(|model|&model.id).collect::<Vec<_>>()
                    }));
                }
            }
            panic!(
                "Selected current provider is unavailable; configured metadata: {}",
                serde_json::to_string(&available).unwrap()
            );
        }
    };
    let body = body.get::<Vec<u8>>(0).unwrap();
    crate::database_fixture::close(database, connection).await;
    let (provider, stored) = read(&body).expect("listed selected provider required");
    assert!(
        provider.enabled && provider.models.iter().any(|value| value.id == model),
        "enabled selected model required"
    );
    (provider, stored)
}

pub(crate) fn load(model: &str) -> (Provider, Secret) {
    let profile = std::env::var_os("SAILRY_ACCEPTANCE_PROFILE").expect("source profile required");
    let db = rusqlite::Connection::open_with_flags(
        std::path::Path::new(&profile).join("storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let (provider, stored) = {
        let mut query = db.prepare("SELECT body FROM providers").unwrap();
        let providers = query.query_map([], |row| row.get::<_, Vec<u8>>(0)).unwrap();
        providers
            .filter_map(|row| read(&row.unwrap()))
            .find(|(provider, _)| {
                provider.enabled && provider.models.iter().any(|value| value.id == model)
            })
            .expect("enabled provider with the selected model required")
    };
    let secret = key(&provider, &stored);
    drop(db);
    (provider, secret)
}

fn read(body: &[u8]) -> Option<(Provider, serde_json::Value)> {
    let stored: serde_json::Value =
        serde_json::from_slice(body).expect("valid provider configuration");
    if !stored["listed"]
        .as_bool()
        .expect("provider visibility required")
    {
        return None;
    }
    let provider: Provider =
        serde_json::from_value(stored["provider"].clone()).expect("valid selected provider");
    Some((provider, stored))
}

fn key(provider: &Provider, stored: &serde_json::Value) -> Secret {
    assert_eq!(
        provider.authentication,
        sailry_protocol::Authentication::ApiKey
    );
    let reference = provider
        .credential
        .as_ref()
        .expect("API credential required");
    let key = stored["authorizations"][reference.id.to_string()]["key"]
        .as_str()
        .expect("selected inline API key required")
        .to_owned();
    Secret::new(key)
}
