use sailry_protocol::{Secret, conversation::Provider};

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
    let secret = secret(&provider, &stored);
    drop(db);
    (provider, secret)
}

pub(super) fn read(body: &[u8]) -> Option<(Provider, serde_json::Value)> {
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

fn secret(provider: &Provider, stored: &serde_json::Value) -> Secret {
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
