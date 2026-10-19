use super::*;
use sailry_protocol::conversation::presets::{Models, Protocol};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

const FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/model-presets.json");

#[test]
fn reads_public_suggestions() {
    let catalog = parse(FIXTURE).unwrap();
    let partner = &catalog.partners[0];
    assert_eq!(partner.id, "example");
    assert_eq!(
        partner.models,
        Models::Static {
            ids: vec!["example-text".into(), "example-vision".into()]
        }
    );
    assert_eq!(
        partner
            .routes
            .iter()
            .map(|route| route.protocol)
            .collect::<Vec<_>>(),
        [
            Protocol::OpenAiResponses,
            Protocol::OpenAiChatCompletions,
            Protocol::Anthropic,
            Protocol::Gemini,
        ]
    );
    let mut external: Value = serde_json::from_slice(FIXTURE).unwrap();
    external["partners"][0]["logo"] = json!("https://example.com/logo.svg");
    assert!(parse(&serde_json::to_vec(&external).unwrap()).is_ok());
    assert!(parse(BUNDLED).unwrap().builtin.is_empty());
    assert_eq!(bundled().builtin.len(), 33);
    for preset in &bundled().builtin {
        validate(preset, true, false).unwrap();
    }
}

#[test]
fn rejects_invalid_documents() {
    let original: Value = serde_json::from_slice(FIXTURE).unwrap();
    let mut cases = vec![];
    for (key, value) in [("version", json!(2)), ("unexpected", json!(true))] {
        let mut invalid = original.clone();
        invalid[key] = value;
        cases.push(invalid);
    }
    for invalid in cases.drain(..) {
        assert!(parse(&serde_json::to_vec(&invalid).unwrap()).is_err());
    }
    for (key, value) in [
        ("id", json!(" ")),
        ("name", json!("")),
        ("routes", json!([])),
        ("logo", json!("icons/providers/../../private.svg")),
        ("homepage", json!("https://user:private@example.com/")),
        ("key_url", json!("http://example.com/keys")),
        (
            "models",
            json!({"kind":"static","data":{"ids":["same", "same"]}}),
        ),
        ("models", json!({"kind":"static","data":{"ids":[""]}})),
    ] {
        let mut invalid = original.clone();
        invalid["partners"][0][key] = value;
        cases.push(invalid);
    }
    let mut duplicate = original.clone();
    duplicate["partners"]
        .as_array_mut()
        .unwrap()
        .push(original["partners"][0].clone());
    assert_eq!(
        parse(&serde_json::to_vec(&duplicate).unwrap())
            .unwrap()
            .partners
            .len(),
        1
    );
    let mut duplicate = original.clone();
    duplicate["partners"][0]["routes"][1] = original["partners"][0]["routes"][0].clone();
    cases.push(duplicate);
    for (key, value) in [
        ("protocol", "unknown"),
        ("endpoint", "https://example.com/v1?api_key=private"),
    ] {
        let mut invalid = original.clone();
        invalid["partners"][0]["routes"][0][key] = json!(value);
        cases.push(invalid);
    }
    for invalid in cases {
        assert!(
            parse(&serde_json::to_vec(&invalid).unwrap())
                .unwrap()
                .partners
                .is_empty()
        );
        invalid_sibling(&original, &invalid);
    }
    assert!(parse(&vec![b' '; BYTE_LIMIT + 1]).is_err());
}

fn invalid_sibling(original: &Value, invalid: &Value) {
    let mut mixed = original.clone();
    mixed["partners"]
        .as_array_mut()
        .unwrap()
        .insert(0, invalid["partners"][0].clone());
    assert_eq!(
        parse(&serde_json::to_vec(&mixed).unwrap())
            .unwrap()
            .partners,
        parse(FIXTURE).unwrap().partners
    );
}

#[tokio::test]
async fn shares_concurrent_loads() {
    let directory = Directory::default();
    let stop = CancellationToken::new();
    let calls = AtomicUsize::new(0);
    let load = || async {
        calls.fetch_add(1, Ordering::SeqCst);
        tokio::task::yield_now().await;
        parse(FIXTURE)
    };
    let (first, second, third) = tokio::join!(
        directory.cached("fixture", &stop, load()),
        directory.cached("fixture", &stop, load()),
        directory.cached("fixture", &stop, load()),
    );
    let expected = first.unwrap();
    assert!(!expected.1);
    assert_eq!(second.unwrap(), expected);
    assert_eq!(third.unwrap(), expected);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn retains_valid_cache_after_failure() {
    let directory = Directory::default();
    let stop = CancellationToken::new();
    let first = directory
        .cached("fixture", &stop, async { parse(FIXTURE) })
        .await
        .unwrap();
    directory.document.lock().await.as_mut().unwrap().attempted = Instant::now() - TTL;
    let fallback = directory
        .cached("fixture", &stop, async { Err(unavailable()) })
        .await
        .unwrap();
    assert!(fallback.1);
    assert_eq!(fallback.0, first.0);
    let cached = directory
        .cached("fixture", &stop, async { panic!("failure must be cached") })
        .await
        .unwrap();
    assert_eq!(cached, fallback);
    directory.document.lock().await.as_mut().unwrap().attempted = Instant::now() - FAILURE_TTL;
    let restored = directory
        .cached("fixture", &stop, async { parse(BUNDLED) })
        .await
        .unwrap();
    assert!(!restored.1);
    assert_eq!(restored.0, parse(BUNDLED).unwrap());
}

#[tokio::test]
async fn isolates_sources_and_cancellation() {
    let directory = Directory::default();
    let stop = CancellationToken::new();
    directory
        .cached("first", &stop, async { parse(FIXTURE) })
        .await
        .unwrap();
    let fallback = directory
        .cached("second", &stop, async { Err(invalid()) })
        .await
        .unwrap();
    assert!(fallback.1);
    assert_eq!(fallback.0, bundled());
    stop.cancel();
    let error = directory
        .cached("second", &stop, std::future::pending())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
}

#[test]
fn permits_distinct_routes_and_local_endpoints() {
    let mut preset = bundled()
        .builtin
        .into_iter()
        .find(|preset| preset.id == "qwen")
        .unwrap();
    validate(&preset, true, false).unwrap();
    preset.auth = Default::default();
    preset.routes[0].endpoint = "http://localhost:11434/v1".into();
    validate(&preset, false, true).unwrap();
    preset.routes.push(preset.routes[0].clone());
    assert!(validate(&preset, false, true).is_err());
}

#[test]
fn separates_sources_and_protected_fields() {
    let mut preset = bundled()
        .builtin
        .into_iter()
        .find(|preset| preset.id == "chat-gpt")
        .unwrap();
    assert!(validate(&preset, false, true).is_err());
    preset.auth = Default::default();
    for key in ["model", "messages", "input", "tools", "stream"] {
        preset.policies =
            vec![serde_json::from_value(json!({"models":["example"],"body":{key:true}})).unwrap()];
        assert!(validate(&preset, false, true).is_err());
    }
}

#[test]
fn preserves_vendor_routes_and_products() {
    let catalog = bundled();
    for (id, endpoint) in [
        ("qwen", "https://dashscope.aliyuncs.com/compatible-mode/v1"),
        ("moonshot", "https://api.moonshot.ai/v1"),
        ("xai", "https://api.x.ai/v1"),
        ("deepseek", "https://api.deepseek.com"),
        ("groq", "https://api.groq.com/openai/v1"),
        ("openrouter", "https://openrouter.ai/api/v1"),
        ("fireworks", "https://api.fireworks.ai/inference/v1"),
        ("together", "https://api.together.xyz/v1"),
        ("mistral", "https://api.mistral.ai/v1"),
        ("perplexity", "https://api.perplexity.ai"),
        ("cerebras", "https://api.cerebras.ai/v1"),
        ("sambanova", "https://api.sambanova.ai/v1"),
        ("minimax", "https://api.minimax.cn/v1"),
        ("doubao", "https://ark.cn-beijing.volces.com/api/v3"),
        ("zhipu", "https://open.bigmodel.cn/api/paas/v4"),
        ("baidu", "https://qianfan.baidubce.com/v2"),
        ("cohere", "https://api.cohere.com/compatibility/v1"),
    ] {
        let preset = catalog
            .builtin
            .iter()
            .find(|preset| preset.id == id)
            .unwrap();
        assert!(
            preset.routes.iter().any(|route| route.endpoint == endpoint),
            "{id}"
        );
        assert_eq!(
            preset.offered,
            [
                "xai", "deepseek", "qwen", "moonshot", "mistral", "minimax", "doubao", "zhipu",
                "baidu", "cohere"
            ]
            .contains(&id)
        );
    }
    for (id, default) in [
        ("qwen", "qwen3.6-plus"),
        ("moonshot", "kimi-for-coding"),
        ("minimax", "MiniMax-M3.1-Flash-Preview"),
        ("zhipu", "glm-5.2"),
        ("doubao", "ark-code-latest"),
    ] {
        let preset = catalog
            .builtin
            .iter()
            .find(|preset| preset.id == id)
            .unwrap();
        let product = preset
            .products
            .iter()
            .find(|product| {
                product.kind == sailry_protocol::conversation::connection::Product::Coding
            })
            .unwrap();
        assert_eq!(product.default_model.as_deref(), Some(default));
        assert_eq!(product.discover, id == "minimax");
        assert!(preset.routes.iter().any(|route| route.product
            == Some(sailry_protocol::conversation::connection::Product::Coding)));
    }
}

#[test]
fn protects_core_request_semantics() {
    for key in [
        "model",
        "messages",
        "input",
        "tools",
        "stream",
        "instructions",
        "system",
        "tool_choice",
        "parallel_tool_calls",
        "functions",
        "function_call",
        "response_format",
    ] {
        let policy =
            serde_json::from_value(json!({"models":["fixture"],"body":{key:true}})).unwrap();
        assert!(validate_policy(&policy).is_err(), "{key}");
    }
    let policy = serde_json::from_value(
        json!({"models":["fixture"],"body":{"thinking":{"clear_thinking":false}}}),
    )
    .unwrap();
    validate_policy(&policy).unwrap();
}

#[test]
fn validates_explicit_brand_displays() {
    let mut original: Value = serde_json::from_slice(FIXTURE).unwrap();
    original["partners"][0]["brand"] = json!("example");
    original["partners"][0]["brand_display"] =
        json!({"label":"Example","locales":{"zh-CN":"示例"}});
    let mut second = original["partners"][0].clone();
    second["id"] = json!("another-route");
    original["partners"].as_array_mut().unwrap().push(second);
    assert_eq!(
        parse(&serde_json::to_vec(&original).unwrap())
            .unwrap()
            .partners
            .len(),
        2
    );
    let mut duplicate = original["partners"][0].clone();
    duplicate["brand"] = json!("another");
    duplicate["brand_display"]["label"] = json!("Duplicate label");
    let mut later = original["partners"][0].clone();
    later["id"] = json!("later");
    later["brand"] = json!("another");
    let mut repeated = original.clone();
    repeated["partners"]
        .as_array_mut()
        .unwrap()
        .extend([duplicate, later]);
    assert_eq!(
        parse(&serde_json::to_vec(&repeated).unwrap())
            .unwrap()
            .partners
            .len(),
        3
    );
    original["partners"][1]["brand_display"]["label"] = json!("Different brand");
    assert_eq!(
        parse(&serde_json::to_vec(&original).unwrap())
            .unwrap()
            .partners
            .len(),
        1
    );
    for value in [
        json!({"label":""}),
        json!({"label":"Example","icon":"invalid icon"}),
    ] {
        let mut invalid = original.clone();
        invalid["partners"].as_array_mut().unwrap().truncate(1);
        invalid["partners"][0]["brand_display"] = value;
        assert!(
            parse(&serde_json::to_vec(&invalid).unwrap())
                .unwrap()
                .partners
                .is_empty()
        );
    }
    let mut invalid = original;
    invalid["partners"].as_array_mut().unwrap().truncate(1);
    invalid["partners"][0]["brand"] = Value::Null;
    assert!(
        parse(&serde_json::to_vec(&invalid).unwrap())
            .unwrap()
            .partners
            .is_empty()
    );
}
