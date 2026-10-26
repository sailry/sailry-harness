use super::*;
use sailry_protocol::conversation::{Model, Page, Part, Status, oauth::Options};

#[path = "codex/completion.rs"]
mod completion;
#[path = "codex/execution.rs"]
mod execution;
#[path = "codex/features.rs"]
mod features;
#[path = "codex/history.rs"]
mod history;
#[path = "codex/lifecycle.rs"]
mod lifecycle;
#[path = "codex/media.rs"]
mod media;
#[path = "codex/search.rs"]
mod search;
#[path = "codex/server.rs"]
pub(super) mod server;
#[path = "codex/streaming.rs"]
mod streaming;
#[path = "codex/transport.rs"]
mod transport;
#[path = "codex/updates.rs"]
mod updates;

const CLIENT: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub(super) const ACCESS: &str = "isolated-codex-access";
pub(super) const REFRESHED: &str = "isolated-codex-renewed-access";
pub(super) const REFRESH: &str = "isolated-codex-refresh";
pub(super) const ROTATED: &str = "isolated-codex-rotated-refresh";
pub(super) const DEVICE: &str = "isolated-codex-device";
pub(super) const ACCOUNT: &str = "isolated-codex-account";
pub(super) const CODE: &str = "isolated-codex-code";
pub(super) const VERIFIER: &str = "isolated-codex-verifier";
const USER_CODE: &str = "CODE-1234";

fn options(version: &str, suffix: &str) -> Options {
    Options::ChatGptOAuth {
        catalog_version: Some(version.into()),
        user_agent: format!("Sailry-Codex-Fixture/{suffix}"),
    }
}

async fn saved(client: &Client, id: ProviderId) -> Provider {
    let Output::Providers(providers) = client
        .execute(client.prepare(Command::ListProviders))
        .await
        .unwrap()
    else {
        panic!("providers expected")
    };
    no_secrets(&providers);
    providers
        .into_iter()
        .find(|provider| provider.id == id)
        .unwrap()
}

pub(super) async fn configured(fixture: &Fixture, options: Option<Options>) -> Provider {
    let mut provider = provider(&fixture.client, Authentication::ChatGptOAuth).await;
    provider.oauth = options;
    provider.models = vec![Model {
        id: "fixture".into(),
        display_name: None,
        context: 8192,
        output: 512,
        vision: true,
        tools: true,
        reasoning: false,
        web_search: false,
        generates: vec![],
        efforts: vec![],
        capabilities: Some(sailry_protocol::conversation::discovery::Capabilities {
            reasoning: Some(true),
            ..Default::default()
        }),
        custom_efforts: false,
        compaction_tokens: None,
        default_effort: Effort::Default,
    }];
    let Output::Provider(provider) = fixture
        .client
        .execute(fixture.client.prepare(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        }))
        .await
        .unwrap()
    else {
        panic!("provider expected")
    };
    let (_, attempt) = begin(&fixture.client, &provider).await;
    let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
    assert_eq!(terminal(&mut stream).await.state, login::State::Connected);
    saved(&fixture.client, provider.id).await
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
