use super::*;

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
        let server = Server::start(vec![reply]).await;
        let provider = provider(api, server.endpoint.clone());
        let model = native(&provider, name, "captured-key", HeaderMap::new());
        let mut input = request(name);
        input.tools[0].parameters = schema.clone();
        collect(&model, input, CancellationToken::new())
            .await
            .unwrap();
        let captured = server.finish().await;
        assert_eq!(captured.len(), 1, "{api:?}");
        assert_eq!(
            declaration(api, &captured[0].body),
            expected(api, &schema),
            "{api:?}"
        );
    }
}

fn expected(api: ModelApi, schema: &Value) -> Value {
    match api {
        ModelApi::Responses => {
            json!({"type":"function","name":"read","description":"Read a file","parameters":schema,"strict":false})
        }
        ModelApi::Anthropic => {
            json!({"name":"read","description":"Read a file","input_schema":schema,"cache_control":{"type":"ephemeral"}})
        }
        ModelApi::Gemini => {
            json!({"name":"read","description":"Read a file","parametersJsonSchema":schema})
        }
        ModelApi::ChatCompletions => json!({
            "name":"read","description":"Read a file","strict":false,
            "parameters":{
                "type":"object","properties":{
                    "action":{"type":"string","enum":["read","write"]},
                    "path":{"type":"string","default":null},
                    "options":{"$ref":"#/$defs/Options"}
                },
                "required":["action"],
                "anyOf":[
                    {"properties":{"action":{"const":"read"}}},
                    {"properties":{"action":{"const":"write"}},"required":["path"]}
                ],
                "$defs":{"Options":{"type":"object","properties":{"timeout":{"type":"integer","format":"uint64"}},"required":[]}}
            }
        }),
        _ => unreachable!(),
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
