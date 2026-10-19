use super::*;
use crate::store::database::Database;
use sailry_protocol::{NodeId, ProviderId, conversation::ModelApi};
use serde_json::json;

fn database() -> (tempfile::TempDir, Database) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .canonicalize()
        .unwrap()
        .join("node.sqlite3");
    let database = Database::open(&path, NodeId([17; 32]), None).unwrap();
    (directory, database)
}

fn package(db: &Connection, digest: &str, value: u32) -> plugin::Info {
    let preset = json!({"id":"example","name":"Example channel","routes":[{"protocol":"openai-chat-completions","endpoint":"http://localhost:11434/v1"}],"policies":[{"models":["fixture"],"body":{"example":value}}]});
    let info:plugin::Info=serde_json::from_value(json!({"summary":{"name":"example","revision":value,"digest":digest,"settings_revision":0,"enabled":true,"version":format!("0.1.{value}"),"description":null},"icon":null,"extension":{"api_version":"v1","model_providers":[preset]},"settings":null})).unwrap();
    db.execute("INSERT OR REPLACE INTO plugins(name,revision,settings_revision,installed,body) VALUES('example',?1,0,1,?2)",params![value,encode(&info).unwrap()]).unwrap();
    db.execute(
        "INSERT OR REPLACE INTO plugin_packages(digest,name,body) VALUES(?1,'example',?2)",
        params![digest, encode(&info).unwrap()],
    )
    .unwrap();
    info
}

fn provider() -> Provider {
    Provider {
        id: ProviderId::new(),
        revision: 1,
        name: "Example".into(),
        api: ModelApi::ChatCompletions,
        authentication: sailry_protocol::Authentication::ApiKey,
        endpoint: "http://localhost:11434/v1".into(),
        options: None,
        oauth: None,
        enabled: true,
        models: vec![],
        default_model: String::new(),
        credential: None,
        preset: Some(PresetRef {
            source: Source::Package {
                name: "example".into(),
            },
            id: "example".into(),
            route: None,
        }),
        binding: None,
        consent: None,
    }
}

#[test]
fn freezes_new_and_inherited_bindings() {
    let (_directory, database) = database();
    let db = &database.connection;
    let first = package(db, &"a".repeat(64), 1);
    let configured = provider();
    let mut admitted = configured.clone();
    capture(db, &mut admitted).unwrap();
    assert_eq!(
        admitted.binding.as_ref().unwrap().package,
        Some(first.summary.reference())
    );
    assert_eq!(
        admitted
            .binding
            .as_ref()
            .unwrap()
            .policies
            .last()
            .unwrap()
            .body["example"],
        1
    );
    let second = package(db, &"b".repeat(64), 2);
    let mut next = configured.clone();
    capture(db, &mut next).unwrap();
    assert_eq!(
        next.binding.as_ref().unwrap().package,
        Some(second.summary.reference())
    );
    assert_eq!(
        next.binding.as_ref().unwrap().policies.last().unwrap().body["example"],
        2
    );
    let inherited = admitted.clone();
    capture(db, &mut admitted).unwrap();
    assert_eq!(admitted, inherited);
    db.execute("UPDATE plugins SET installed=0 WHERE name='example'", [])
        .unwrap();
    assert_eq!(
        capture(db, &mut configured.clone()).unwrap_err().code,
        ErrorCode::NotConfigured
    );
    capture(db, &mut admitted).unwrap();
    assert_eq!(admitted, inherited);
    assert!(configured.binding.is_none());
    assert!(validate(db, &admitted).is_err());
    db.execute(
        "DELETE FROM plugin_packages WHERE digest=?1",
        [first.summary.digest],
    )
    .unwrap();
    assert_eq!(
        verify(db, &admitted).unwrap_err().code,
        ErrorCode::NotConfigured
    );
}

#[test]
fn retains_partner_declarations_across_reopen() {
    let (directory, database) = database();
    let catalog = crate::providers::presets::parse(include_bytes!(
        "../../../../tests/fixtures/model-presets.json"
    ))
    .unwrap();
    let first = list(&database.connection, Some(&catalog), false).unwrap();
    assert!(
        first
            .entries
            .iter()
            .any(|entry| entry.source == Source::Partner)
    );
    drop(database);
    let database = Database::open(
        &directory
            .path()
            .canonicalize()
            .unwrap()
            .join("node.sqlite3"),
        NodeId([17; 32]),
        None,
    )
    .unwrap();
    assert_eq!(
        list(&database.connection, None, true).unwrap().entries,
        first.entries
    );
    let mut provider = provider();
    provider.api = ModelApi::Responses;
    provider.preset = Some(PresetRef {
        source: Source::Partner,
        id: "example".into(),
        route: None,
    });
    capture(&database.connection, &mut provider).unwrap();
    assert!(provider.binding.is_some());
    assert!(provider.binding.as_ref().unwrap().package.is_none());
}

#[test]
fn refreshes_roles_without_changing_captured_configuration() {
    use sailry_protocol::{
        RoleId,
        conversation::consent::{Approval, Origin, Scope, Scopes},
        role,
    };
    let (_directory, database) = database();
    let db = &database.connection;
    let mut info = package(db, &"a".repeat(64), 1);
    info.origin = Some(plugin::Origin::Bundled);
    let mut extension = serde_json::to_value(info.extension.as_ref().unwrap()).unwrap();
    extension["actions"] = json!(["models.provide"]);
    extension["model_providers"][0]["routes"][0]["protocol"] = "plugin".into();
    extension["model_providers"][0]["adapter"] = "example".into();
    extension["model_adapters"] = json!([{"id":"example","script":{"entry":"dev.sailry.platform/adapter.js"},"origins":[],"framing":"sse","capabilities":{"tools":true}}]);
    info.extension = Some(serde_json::from_value(extension).unwrap());
    db.execute(
        "UPDATE plugins SET body=?1 WHERE name='example'",
        [encode(&info).unwrap()],
    )
    .unwrap();
    db.execute(
        "UPDATE plugin_packages SET body=?1 WHERE digest=?2",
        params![encode(&info).unwrap(), info.summary.digest],
    )
    .unwrap();
    let mut configured = provider();
    configured.api = ModelApi::Plugin;
    configured.consent = Some(Box::new(Approval {
        package: info.summary.reference(),
        origin: Some(Origin::Bundled),
        scopes: Scopes {
            messages: true,
            credentials: false,
        },
    }));
    crate::store::providers::configuration::publish(db, &configured).unwrap();
    let mut admitted = configured.clone();
    capture(db, &mut admitted).unwrap();
    let mut roster = role::Snapshot {
        profiles: vec![role::Profile {
            id: RoleId::new(),
            revision: 1,
            key: "analyst".into(),
            name: "Analyst".into(),
            appearance: None,
            description: String::new(),
            read_only: true,
            model: Some(role::Model {
                provider: configured.id,
                model: "fixture".into(),
                effort: None,
            }),
            max_turns: None,
            skills: Vec::new(),
            instructions: String::new(),
        }],
        providers: vec![admitted.clone()],
    };
    roster.validate().unwrap();
    configured.consent.as_mut().unwrap().scopes = Scopes {
        messages: false,
        credentials: true,
    };
    configured.endpoint = "https://changed.invalid".into();
    crate::store::providers::configuration::publish(db, &configured).unwrap();
    crate::store::sessions::roles::refresh(db, &mut roster).unwrap();
    assert_eq!(roster.providers[0].endpoint, admitted.endpoint);
    let original = admitted.clone();
    capture(db, &mut admitted).unwrap();
    assert_eq!(admitted, original);
    assert!(crate::store::providers::consent::check(&admitted, Scope::Messages).is_ok());
    crate::store::providers::consent::check(&admitted, Scope::Credentials).unwrap();
    configured.consent.as_mut().unwrap().scopes.messages = true;
    crate::store::providers::configuration::publish(db, &configured).unwrap();
    roster.providers[0] = original.clone();
    crate::store::sessions::roles::refresh(db, &mut roster).unwrap();
    assert_eq!(roster.providers[0].endpoint, original.endpoint);
    assert!(
        roster.providers[0]
            .consent
            .as_ref()
            .unwrap()
            .scopes
            .credentials
    );
    assert!(!original.consent.as_ref().unwrap().scopes.credentials);
}

#[test]
fn retains_saved_policy_after_partner_removal() {
    let (_directory, database) = database();
    let db = &database.connection;
    let mut catalog = crate::providers::presets::parse(include_bytes!(
        "../../../../tests/fixtures/model-presets.json"
    ))
    .unwrap();
    catalog.partners[0].auth = Auth::ApiKey {
        header: Some("X-Example-Key".into()),
        prefix: Some("Token ".into()),
    };
    let gemini = catalog.partners[0].routes.pop().unwrap();
    assert_eq!(gemini.protocol, Protocol::Gemini);
    assert_eq!(
        catalog.partners[0]
            .routes
            .iter()
            .map(|route| route.protocol)
            .collect::<Vec<_>>(),
        [
            Protocol::OpenAiResponses,
            Protocol::OpenAiChatCompletions,
            Protocol::Anthropic
        ]
    );
    crate::providers::presets::validate(&catalog.partners[0], false, false).unwrap();
    let mut unsupported = catalog.partners[0].clone();
    unsupported.routes.push(gemini);
    assert_eq!(
        crate::providers::presets::validate(&unsupported, false, false)
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    catalog.partners[0].policies = vec![
        serde_json::from_value(json!({"models":["fixture"],"body":{"thinking":{"enabled":true}}}))
            .unwrap(),
    ];
    list(db, Some(&catalog), false).unwrap();
    let mut selected = provider();
    selected.preset.as_mut().unwrap().source = Source::Partner;
    save_policy(db, &mut selected, None).unwrap();
    let original = selected.clone();
    assert_eq!(
        selected
            .binding
            .as_ref()
            .unwrap()
            .key_injection
            .as_ref()
            .unwrap()
            .header,
        "X-Example-Key"
    );
    catalog.partners.clear();
    list(db, Some(&catalog), false).unwrap();
    capture(db, &mut selected).unwrap();
    assert_eq!(selected, original);
    selected.name = "Renamed saved channel".into();
    save_policy(db, &mut selected, Some(&original)).unwrap();
    assert_eq!(selected.binding, original.binding);
    selected.binding.as_mut().unwrap().package = Some(plugin::Reference {
        name: "other".into(),
        digest: "a".repeat(64),
        settings_revision: 0,
    });
    assert!(partner_policy(&selected).is_err());
    let mut legacy = original;
    legacy.binding = None;
    capture(db, &mut legacy).unwrap();
    assert!(legacy.binding.unwrap().key_injection.is_none());
}

#[test]
fn discards_only_a_corrupt_suggestion_cache() {
    let (directory, database) = database();
    database
        .connection
        .execute(
            "INSERT INTO channel_presets(singleton,body) VALUES(1,?1)",
            [b"not JSON".as_slice()],
        )
        .unwrap();
    let before: i64 = database
        .connection
        .query_row("SELECT count(*) FROM providers", [], |row| row.get(0))
        .unwrap();
    drop(database);
    let database = Database::open(
        &directory
            .path()
            .canonicalize()
            .unwrap()
            .join("node.sqlite3"),
        NodeId([17; 32]),
        None,
    )
    .unwrap();
    assert_eq!(
        database
            .connection
            .query_row("SELECT count(*) FROM channel_presets", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        database
            .connection
            .query_row("SELECT count(*) FROM providers", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        before
    );
    assert!(
        !list(&database.connection, None, true)
            .unwrap()
            .entries
            .is_empty()
    );
}
