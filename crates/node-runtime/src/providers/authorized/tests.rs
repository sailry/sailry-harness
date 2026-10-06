use super::*;
use serde_json::json;

#[test]
fn preserves_native_metadata() {
    let models = parse(Authentication::ChatGpt, ModelApi::Responses, &json!({"models":[
        {"slug":"known","visibility":"list","display_name":"Known model","context_window":4096,"max_output_tokens":128,"input_modalities":["text","image"],"supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}],"default_reasoning_level":"high"},
        {"slug":"future","visibility":"list","display_name":"Future model","supported_reasoning_levels":[{"effort":"new-native"}],"default_reasoning_level":"new-native"}
    ]})).unwrap();
    assert_eq!(models[0].id, "known");
    assert_eq!(models[0].display_name.as_deref(), Some("Known model"));
    assert_eq!(models[1].context, None);
    let future = models[1].capabilities.as_ref().unwrap();
    assert_eq!(future.reasoning, Some(true));
    assert_eq!(future.efforts, Some(vec![]));
    assert_eq!(future.default_effort, None);
    assert_eq!(models[0].context, Some(4096));
    let known = models[0].capabilities.as_ref().unwrap();
    assert_eq!(known.vision, Some(true));
    assert_eq!(known.tools, None);
    assert_eq!(known.efforts, Some(vec![Effort::Low, Effort::High]));
    assert_eq!(known.default_effort, Some(Effort::High));
}

#[test]
fn keeps_all_returned_models() {
    let mut entries = [None, Some("list"), Some("hide"), Some("unknown")].into_iter().enumerate().map(|(index, visibility)| {
        let mut entry = json!({"slug":format!("catalog-fixture-{index}"),"minimal_client_version":"0.153.0"});
        if let Some(visibility) = visibility { entry["visibility"] = json!(visibility); }
        entry
    }).collect::<Vec<_>>();
    let expected = entries
        .iter()
        .map(|entry| entry["slug"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    entries.push(entries[0].clone());
    let models = parse(
        Authentication::ChatGpt,
        ModelApi::Responses,
        &json!({"models":entries}),
    )
    .unwrap();
    assert_eq!(
        models
            .iter()
            .map(|model| model.id.clone())
            .collect::<Vec<_>>(),
        expected
    );
    for model in models {
        assert_eq!(model.display_name, None);
        assert_eq!(model.context, None);
        assert_eq!(model.output, None);
        assert_eq!(model.capabilities.as_ref().unwrap().tools, None);
    }
}

#[test]
fn separates_copilot_surfaces() {
    let value = json!({"data":[
        {"id":"responses","capabilities":{"type":"chat","limits":{"max_context_window_tokens":8192,"max_prompt_tokens":4096,"max_output_tokens":1024},"supports":{"vision":false,"tool_calls":true,"reasoning_effort":["none","low","high"]}},"supported_endpoints":["/responses"]},
        {"id":"chat","capabilities":{"type":"chat"}},
        {"id":"embedding","capabilities":{"type":"embeddings"},"supported_endpoints":["/responses"]},
        {"id":"messages","capabilities":{"type":"chat"},"supported_endpoints":["/v1/messages"]}
    ]});
    let models = parse(Authentication::Copilot, ModelApi::Responses, &value).unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "responses");
    assert_eq!(models[0].context, Some(8192));
    assert_eq!(models[0].output, Some(1024));
    let capabilities = models[0].capabilities.as_ref().unwrap();
    assert_eq!(capabilities.tools, Some(true));
    assert_eq!(capabilities.vision, Some(false));
    assert_eq!(
        capabilities.efforts,
        Some(vec![Effort::Disabled, Effort::Low, Effort::High])
    );
    let chat = parse(Authentication::Copilot, ModelApi::ChatCompletions, &value).unwrap();
    assert_eq!(chat.len(), 1);
    assert_eq!(chat[0].id, "chat");
    assert_eq!(chat[0].context, None);
}

#[test]
fn rejects_conflicting_metadata() {
    for value in [
        json!({"models":[{"slug":"same","display_name":"Same","visibility":"list","context_window":2},{"slug":"same","display_name":"Same","visibility":"list","context_window":4}]}),
        json!({"models":[{"slug":"same","display_name":"First","visibility":"list"},{"slug":"same","display_name":"Second","visibility":"list"}]}),
        json!({"models":[{"slug":"bad","display_name":"Bad","visibility":"list","context_window":2,"max_output_tokens":4}]}),
        json!({"models":[{"slug":"bad","display_name":"Bad","visibility":"list","supported_reasoning_levels":[{"effort":"high"},{"effort":"high"}]}]}),
        json!({"models":[{"slug":"bad","display_name":"Bad","visibility":"list","supported_reasoning_levels":[{"effort":"high"}],"default_reasoning_level":"low"}]}),
        json!({"models":[{"slug":"bad","display_name":"Bad","visibility":"list","input_modalities":[false]}]}),
        json!({"models":[{"slug":"bad","display_name":"","visibility":"list"}]}),
    ] {
        assert!(parse(Authentication::ChatGpt, ModelApi::Responses, &value).is_err());
    }
    assert!(parse(Authentication::Copilot, ModelApi::Responses, &json!({"data":[{"id":"bad","capabilities":{"type":"chat","supports":{"vision":"true"}},"supported_endpoints":["/responses"]}]})).is_err());
}
