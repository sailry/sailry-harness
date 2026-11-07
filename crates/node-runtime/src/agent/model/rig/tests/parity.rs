use super::*;
use goose_provider_types::{
    base::collect_stream, conversation::message::Message as LegacyMessage, model::ModelConfig,
};

#[tokio::test]
async fn preserves_existing_tool_schemas() {
    let schema = json!({
        "type":"object",
        "properties":{
            "action":{"type":"string","enum":["read","write"]},
            "path":{"type":["string","null"],"default":null},
            "options":{"$ref":"#/$defs/Options"}
        },
        "required":["action"],
        "oneOf":[
            {"properties":{"action":{"const":"read"}}},
            {"properties":{"action":{"const":"write"}},"required":["path"]}
        ],
        "$defs":{"Options":{"type":"object","properties":{
            "timeout":{"anyOf":[{"type":"integer","format":"uint64"},{"type":"null"}]}
        }}}
    });
    for (api, name, reply) in [
        (ModelApi::Responses, "fixture", fixtures::responses(false)),
        (ModelApi::ChatCompletions, "fixture", fixtures::chat()),
        (
            ModelApi::Anthropic,
            "claude-opus-4-8",
            fixtures::anthropic(false),
        ),
        (
            ModelApi::Gemini,
            "gemini-3-flash-preview",
            fixtures::gemini(false),
        ),
    ] {
        let server = Server::start(vec![reply.clone(), reply]).await;
        let provider = provider(api, server.endpoint.clone());
        let registry = registry::Registry::default();
        let legacy = crate::agent::model::factory::native(
            &registry,
            &provider,
            name,
            SessionId::new(),
            "captured-key",
            HeaderMap::new(),
        )
        .unwrap();
        let legacy_tool =
            rmcp::model::Tool::new("read", "Read a file", schema.as_object().unwrap().clone());
        let config = ModelConfig::new(name).with_max_tokens(Some(512));
        collect_stream(
            legacy
                .stream(
                    &config,
                    "Captured instructions",
                    &[LegacyMessage::user().with_text("Read the file")],
                    &[legacy_tool],
                )
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        let model = native(&provider, name, "captured-key", HeaderMap::new());
        let mut input = request(name);
        input.tools[0].parameters = schema.clone();
        collect(&model, input, CancellationToken::new())
            .await
            .unwrap();
        let captured = server.finish().await;
        assert_eq!(captured.len(), 2, "{api:?}");
        assert_eq!(
            declaration(api, &captured[1].body),
            declaration(api, &captured[0].body),
            "{api:?}"
        );
    }
}

fn declaration(api: ModelApi, body: &Value) -> Value {
    match api {
        ModelApi::ChatCompletions => body["tools"][0]["function"].clone(),
        ModelApi::Responses | ModelApi::Anthropic => body["tools"][0].clone(),
        ModelApi::Gemini => {
            let tools = if body["tools"].is_array() {
                &body["tools"][0]
            } else {
                &body["tools"]
            };
            tools["functionDeclarations"][0].clone()
        }
        _ => unreachable!(),
    }
}
