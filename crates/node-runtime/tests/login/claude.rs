use super::*;
use sailry_protocol::conversation::{Page, Part, Status, oauth::Options};
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};

#[path = "claude/lifecycle.rs"]
mod lifecycle;
#[path = "claude/renewals.rs"]
mod renewals;
#[path = "claude/server.rs"]
pub(super) mod server;

const CLIENT: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const REDIRECT: &str = "http://127.0.0.1:53692/callback";
const COPY_REDIRECT: &str = "https://platform.claude.com/oauth/code/callback";
const ACCESS: &str = "isolated-claude-access";
const REFRESH: &str = "isolated-claude-refresh";
const REFRESHED: &str = "isolated-claude-renewed-access";
const ROTATED: &str = "isolated-claude-rotated-refresh";
const CODE: &str = "isolated-claude-code";
const INSTRUCTION: &str = "Keep the isolated Claude user instruction";
const ANSWER: &str = "Authorized Claude response";

fn redacted(value: &impl serde::Serialize) {
    super::no_secrets(value);
    let body = serde_json::to_string(value).unwrap();
    for secret in [ACCESS, REFRESH, REFRESHED, ROTATED, CODE] {
        assert!(
            !body.contains(secret),
            "private authorization escaped its owner"
        );
    }
}

async fn provider(client: &Client, options: Option<Options>) -> Provider {
    let provider = Provider {
        request_max_retries: None,
        stream_max_retries: None,
        preset: None,
        binding: None,
        consent: None,
        id: ProviderId::new(),
        revision: 0,
        name: "Claude authorization fixture".into(),
        api: ModelApi::Anthropic,
        authentication: Authentication::ClaudeOAuth,
        endpoint: "https://api.anthropic.com".into(),
        options: None,
        oauth: options,
        enabled: true,
        models: vec![],
        default_model: "fixture".into(),
        credential: None,
    };
    let Output::Provider(provider) = client
        .execute(client.prepare(Command::PutProvider {
            provider,
            expected_revision: 0,
        }))
        .await
        .unwrap()
    else {
        panic!("provider expected")
    };
    provider
}

async fn saved(client: &Client, id: ProviderId) -> Provider {
    let Output::Providers(providers) = client
        .execute(client.prepare(Command::ListProviders))
        .await
        .unwrap()
    else {
        panic!("providers expected")
    };
    redacted(&providers);
    providers
        .into_iter()
        .find(|provider| provider.id == id)
        .unwrap()
}

async fn begin(
    client: &Client,
    provider: &Provider,
    redirect: Option<&str>,
) -> (Request, login::Attempt, String) {
    let request = client.prepare(Command::BeginProviderLogin {
        provider: provider.id,
        expected_revision: provider.revision,
        redirect: redirect.map(str::to_owned),
    });
    let admission = client.dispatch(request.clone()).await.unwrap();
    assert!(admission.receipt.durable);
    let Output::ProviderLogin(attempt) = admission.completion.await.unwrap().unwrap() else {
        panic!("attempt expected")
    };
    assert_eq!(attempt.id, request.id);
    let url = super::browser(client, &attempt).await;
    (request, attempt, url)
}

fn callback(authorization: &str, redirect: Option<&str>) -> Secret {
    let url = url::Url::parse(authorization).unwrap();
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    if let Some(redirect) = redirect {
        let mut url = url::Url::parse(redirect).unwrap();
        url.query_pairs_mut()
            .extend_pairs([("code", CODE), ("state", state.as_str())]);
        Secret::new(url.into())
    } else {
        Secret::new(format!("{CODE}#{state}"))
    }
}

pub(super) async fn connected(fixture: &Fixture) -> Provider {
    let provider = provider(&fixture.client, None).await;
    let (_, attempt, url) = begin(&fixture.client, &provider, Some(REDIRECT)).await;
    super::complete(&fixture.client, &attempt, callback(&url, Some(REDIRECT))).await;
    let mut updates = fixture.client.subscribe_login(attempt.id).await.unwrap();
    let update = super::terminal(&mut updates).await;
    redacted(&update);
    assert_eq!(update.state, login::State::Connected);
    assert_eq!(update.model_error, None);
    saved(&fixture.client, provider.id).await
}

fn grant(fixture: &Fixture, provider: &Provider) -> Value {
    let db = rusqlite::Connection::open_with_flags(
        fixture.node.profile().join("storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let stored: Vec<u8> = db
        .query_row(
            "SELECT body FROM providers WHERE id=?1",
            [provider.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    let stored: Value = serde_json::from_slice(&stored).unwrap();
    let reference = provider.credential.as_ref().unwrap();
    serde_json::from_str(
        stored["authorizations"][reference.id.to_string()]["key"]
            .as_str()
            .unwrap(),
    )
    .unwrap()
}

fn expire(fixture: &Fixture, provider: &Provider) {
    let db =
        rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3")).unwrap();
    let stored: Vec<u8> = db
        .query_row(
            "SELECT body FROM providers WHERE id=?1",
            [provider.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    let mut stored: Value = serde_json::from_slice(&stored).unwrap();
    let reference = provider.credential.as_ref().unwrap();
    let key = &mut stored["authorizations"][reference.id.to_string()]["key"];
    let mut grant: Value = serde_json::from_str(key.as_str().unwrap()).unwrap();
    grant["expires_at_ms"] = json!((super::future_expiry() - 3601) * 1000);
    *key = json!(grant.to_string());
    db.execute(
        "UPDATE providers SET body=?1 WHERE id=?2",
        rusqlite::params![
            serde_json::to_vec(&stored).unwrap(),
            provider.id.to_string()
        ],
    )
    .unwrap();
}

fn public_rows(fixture: &Fixture, authorization: &str) {
    let url = url::Url::parse(authorization).unwrap();
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    let db = rusqlite::Connection::open_with_flags(
        fixture.node.profile().join("storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut query = db.prepare(
        "SELECT CAST(body AS TEXT) FROM requests UNION ALL SELECT CAST(result AS TEXT) FROM requests UNION ALL SELECT CAST(body AS TEXT) FROM events UNION ALL SELECT CAST(body AS TEXT) FROM agent_events"
    ).unwrap();
    for body in query.query_map([], |row| row.get::<_, String>(0)).unwrap() {
        let body = body.unwrap();
        redacted(&body);
        assert!(
            !body.contains(&state),
            "PKCE context escaped its login owner"
        );
    }
}

fn answer(page: &Page, turn: TurnId) -> String {
    page.entries
        .iter()
        .filter(|entry| entry.turn == turn && entry.author != "user")
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| match part {
            Part::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}
