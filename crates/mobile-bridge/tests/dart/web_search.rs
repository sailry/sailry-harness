use super::native::server as native;
use super::{configuration::support, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{conversation::ModelApi, *};

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn responses_preserve_sources_and_settings() {
    check(ModelApi::Responses);
}

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn anthropic_preserves_encrypted_history() {
    check(ModelApi::Anthropic);
}

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn gemini_preserves_suggestions() {
    check(ModelApi::Gemini);
}

fn check(api: ModelApi) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("project")).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let gemini = (api == ModelApi::Gemini)
        .then(|| runtime.block_on(native::Server::start(api, native::Reply::Grounding)));
    let server = runtime.block_on(async {
        if api == ModelApi::Anthropic {
            support::Server::anthropic(support::anthropic::Reply::Search).await
        } else {
            support::Server::web_search().await
        }
    });
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        gemini
            .as_ref()
            .map_or(&server.endpoint, |server| &server.endpoint),
        "ffi-web-search",
    ));
    let client = Client::new(node.local());
    let selected = runtime.block_on(async {
        let Output::Plugin(package) = client
            .execute(client.prepare(Command::ReadPlugin {
                name: "web-search".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("web search package expected")
        };
        client
            .execute(client.prepare(Command::SetPluginEnabled {
                name: package.summary.name,
                expected_revision: package.summary.revision,
                enabled: true,
            }))
            .await
            .unwrap();
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let mut provider = snapshot.providers[0].clone();
        provider.api = api;
        provider.models[0].tools = false;
        provider.models[0].web_search = true;
        let Output::Provider(provider) = client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        provider
    });
    let invitation = node.link().invite().unwrap();
    let child = super::command("web_search.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSION", sessions[0].to_string())
        .env("SAILRY_PROVIDER", serde_json::to_string(&selected).unwrap())
        .spawn()
        .unwrap();
    assert!(super::wait(child).success());
    let pages = runtime.block_on(tools::pages(&client, &sessions));
    assert_eq!(
        pages[0]
            .entries
            .iter()
            .flat_map(|entry| &entry.citations)
            .count(),
        if api == ModelApi::Gemini { 1 } else { 2 }
    );
    runtime.block_on(node.shutdown()).unwrap();
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    assert_eq!(
        runtime.block_on(tools::pages(&Client::new(node.local()), &sessions)),
        pages
    );
    runtime.block_on(node.shutdown()).unwrap();
    if let Some(gemini) = gemini {
        let requests = gemini.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            requests[0].body["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["google_search"].is_object())
        );
        assert!(
            !requests[1].body["tools"]
                .as_array()
                .is_some_and(|tools| tools.iter().any(|tool| tool["google_search"].is_object()))
        );
        assert!(
            requests[1].body["contents"]
                .to_string()
                .contains(native::ANSWER)
        );
        return;
    }
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let kind = if api == ModelApi::Anthropic {
        "web_search_20250305"
    } else {
        "web_search"
    };
    assert!(
        requests[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["type"] == kind)
    );
    assert!(
        !requests[1]["tools"]
            .as_array()
            .is_some_and(|tools| tools.iter().any(|tool| tool["type"] == kind))
    );
    assert!(
        requests[1][if api == ModelApi::Anthropic {
            "messages"
        } else {
            "input"
        }]
        .to_string()
        .contains(support::web_search::FIRST)
    );
    if api == ModelApi::Anthropic {
        assert!(
            requests[1]["messages"]
                .to_string()
                .contains(support::anthropic::ENCRYPTED)
        );
        assert!(
            requests[1]["messages"]
                .to_string()
                .contains(support::anthropic::INDEX)
        );
        let public = serde_json::to_string(&pages).unwrap();
        assert!(!public.contains(support::anthropic::ENCRYPTED));
        assert!(!public.contains(support::anthropic::INDEX));
    }
    assert!(
        !serde_json::to_string(&pages)
            .unwrap()
            .contains("isolated-ffi-configuration-credential")
    );
}
