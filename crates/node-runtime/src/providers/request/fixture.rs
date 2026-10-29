use super::*;

pub(crate) const SESSION: &str = "11111111-1111-4111-8111-111111111111";
pub(crate) const REQUEST: &str = "22222222-2222-4222-8222-222222222222";

pub(crate) fn provider(authentication: Authentication, api: ModelApi) -> Provider {
    serde_json::from_value(json!({
        "id":"33333333-3333-4333-8333-333333333333", "revision":1,
        "name":"Authorization fixture", "api":api, "authentication":authentication,
        "endpoint":"https://model.example.test/gateway", "enabled":true,
        "models":[], "default_model":"fixture-model", "credential":null
    }))
    .unwrap()
}

pub(crate) fn metadata<'a>(
    model: &'a str,
    system: &'a str,
    tool_names: &'a [&'a str],
) -> Metadata<'a> {
    Metadata {
        model,
        system,
        tool_names,
        session: SESSION.parse().unwrap(),
        request: REQUEST.parse().unwrap(),
        delegated: false,
        user: true,
        vision: true,
    }
}

pub(crate) fn credentials(authentication: Authentication) -> Credentials {
    if authentication == Authentication::ApiKey {
        return Credentials::ApiKey(Some(Secret::new("fixed-access".into())));
    }
    let mut value = json!({
        "kind":authentication, "access":"fixed-access", "refresh":"fixed-refresh",
        "expires_at_ms":1800000000000u64, "renewal_pending":false
    });
    let options = match authentication {
        Authentication::ApiKey => unreachable!(),
        Authentication::ChatGpt => {
            value["client_id"] = json!("oaiapp_fixture");
            value["subject"] = json!("subject");
            value["id_token"] = json!("fixed-identity");
            value["scopes"] = json!(["resource.invoke", "chatgpt.tokens.use.direct"]);
            Options::ChatGpt {
                catalog_version: None,
                user_agent: "CapturedAgent/1".into(),
            }
        }
        Authentication::ChatGptOAuth => {
            value["subject"] = json!("subject");
            value["account_id"] = json!("fixed-account");
            value["id_token"] = json!("fixed-identity");
            Options::ChatGptOAuth {
                catalog_version: Some("0.161.0".into()),
                user_agent: "CapturedAgent/1".into(),
            }
        }
        Authentication::ClaudeOAuth => Options::ClaudeOAuth {
            user_agent: "CapturedAgent/1".into(),
        },
        Authentication::Copilot => {
            value["github"] = json!("fixed-github");
            value["endpoint"] = json!("https://api.githubcopilot.com");
            Options::Copilot {
                user_agent: "CapturedAgent/1".into(),
                editor_version: "CapturedEditor/1".into(),
                editor_plugin_version: "CapturedPlugin/1".into(),
            }
        }
        Authentication::Plugin => {
            value = json!({"kind":"plugin", "record":{
                "target":{
                    "provider":"33333333-3333-4333-8333-333333333333", "package":"request-fixture",
                    "digest":null, "origin":{"kind":"bundled"}, "flow":"login",
                    "endpoint":"https://model.example.test/gateway",
                    "authorization":{"kind":"token_exchange", "token":{"url":"https://auth.example.test/token"}},
                    "refresh":null,
                    "inject":[{"header":"x-plugin-key", "template":"token {access}"}, {"header":"x-account", "template":"{extra.account}"}],
                    "fields":{}, "scopes":[]
                },
                "issued":{"access":"fixed-access", "refresh":"fixed-refresh", "expires_at_ms":1800000000000u64, "extra":{"account":"private-account"}}
            }});
            Options::Plugin {
                package: sailry_protocol::plugin::Reference {
                    name: "request-fixture".into(),
                    digest: "a".repeat(64),
                    settings_revision: 0,
                },
                flow: "login".into(),
                endpoint: "https://model.example.test/gateway".into(),
                consent: None,
                origin: Some(sailry_protocol::conversation::consent::Origin::Bundled),
            }
        }
    };
    let grant = Grant::decode(authentication, &Secret::new(value.to_string())).unwrap();
    Credentials::Grant {
        grant: Box::new(grant),
        options,
    }
}

pub(crate) fn headers(authentication: Authentication) -> Value {
    match authentication {
        Authentication::ApiKey => json!({"authorization":"Bearer fixed-access"}),
        Authentication::ChatGpt => {
            json!({"authorization":"Bearer fixed-access", "user-agent":"CapturedAgent/1"})
        }
        Authentication::ChatGptOAuth => json!({
            "authorization":"Bearer fixed-access", "user-agent":"CapturedAgent/1", "originator":"codex_cli_rs",
            "version":"0.161.0", "chatgpt-account-id":"fixed-account", "session-id":SESSION,
            "x-codex-routing-hint":"model=fixture-model"
        }),
        Authentication::ClaudeOAuth => json!({
            "authorization":"Bearer fixed-access", "user-agent":"CapturedAgent/1", "anthropic-version":"2023-06-01",
            "anthropic-beta":"claude-code-20250219,oauth-2025-04-20", "anthropic-dangerous-direct-browser-access":"true", "x-app":"cli"
        }),
        Authentication::Copilot => json!({
            "authorization":"Bearer fixed-access", "user-agent":"CapturedAgent/1", "editor-version":"CapturedEditor/1",
            "editor-plugin-version":"CapturedPlugin/1", "copilot-integration-id":"vscode-chat", "x-github-api-version":"2025-04-01",
            "openai-intent":"conversation-agent", "x-initiator":"user", "x-request-id":REQUEST, "copilot-vision-request":"true"
        }),
        Authentication::Plugin => {
            json!({"x-plugin-key":"token fixed-access", "x-account":"private-account"})
        }
    }
}

pub(crate) fn header_value(headers: &HeaderMap) -> Value {
    serde_json::to_value(
        headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.to_str().unwrap()))
            .collect::<BTreeMap<_, _>>(),
    )
    .unwrap()
}
