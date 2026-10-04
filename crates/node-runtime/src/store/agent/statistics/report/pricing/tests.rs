use super::*;
use serde_json::json;

#[test]
fn prices_token_categories_and_tiers() {
    let cost = json!({"input":2,"output":8,"cache_read":0.5,"cache_write":3,"reasoning":10,
        "tiers":[{"tier":{"type":"context","size":100},"input":4,"output":16,"cache_read":1,"cache_write":6,"reasoning":20}]});
    let mut usage = UsageMetadata {
        prompt_token_count: 100,
        candidates_token_count: 50,
        cache_read_input_token_count: Some(20),
        cache_creation_input_token_count: Some(10),
        thinking_token_count: Some(5),
        ..Default::default()
    };
    assert_eq!(estimate(&cost, &usage), Some(priced(140, 410, 10, 30)));
    usage.prompt_token_count = 101;
    assert_eq!(estimate(&cost, &usage), Some(priced(284, 820, 20, 60)));
    usage.prompt_token_count = 0;
    assert_eq!(estimate(&cost, &usage), None);
}

#[test]
fn distinguishes_unknown_and_free_prices() {
    let usage = UsageMetadata {
        prompt_token_count: 100,
        candidates_token_count: 50,
        cache_read_input_token_count: Some(20),
        ..Default::default()
    };
    assert_eq!(estimate(&json!({"input":2,"output":8}), &usage), None);
    assert_eq!(
        estimate(&json!({"input":0,"output":0,"cache_read":0}), &usage),
        Some(priced(0, 0, 0, 0))
    );
    assert_eq!(
        estimate(&json!({"input":-1,"output":8,"cache_read":1}), &usage),
        None
    );
    assert_eq!(estimate(&json!({}), &UsageMetadata::default()), None);
    assert_eq!(micros(f64::INFINITY), None);
    assert_eq!(micros(-0.1), None);
    assert_eq!(micros(0.1234567), Some(123457));
}

#[test]
fn prices_disjoint_audio_tokens() {
    let cost = json!({"input":2,"output":8,"input_audio":20,"output_audio":40});
    let mut usage = UsageMetadata {
        prompt_token_count: 100,
        candidates_token_count: 50,
        audio_input_token_count: Some(20),
        audio_output_token_count: Some(10),
        ..Default::default()
    };
    assert_eq!(estimate(&cost, &usage), Some(priced(560, 720, 0, 0)));
    let mut unknown = usage.clone();
    unknown.audio_input_token_count = None;
    assert_eq!(estimate(&cost, &unknown), None);
    usage.cache_read_input_token_count = Some(10);
    assert_eq!(estimate(&cost, &usage), None);
}

#[test]
fn prices_custom_endpoints() {
    assert_eq!(
        supplier(
            Some("responses"),
            Some("api_key"),
            Some("https://api.openai.com/v1/")
        ),
        Some("openai")
    );
    assert_eq!(
        supplier(
            Some("responses"),
            Some("chat_gpt"),
            Some("https://api.openai.com/v1")
        ),
        None
    );
    assert_eq!(
        supplier(
            Some("responses"),
            Some("api_key"),
            Some("https://proxy.invalid/v1")
        ),
        Some("openai")
    );
    assert_eq!(
        supplier(
            Some("anthropic"),
            Some("api_key"),
            Some("https://proxy.invalid/v1")
        ),
        Some("anthropic")
    );
}

fn priced(input: u64, output: u64, cache_read: u64, cache_write: u64) -> Cost {
    Cost {
        usd_micros: input + output + cache_read + cache_write,
        responses: 1,
        breakdown: Some(CostBreakdown {
            input,
            output,
            cache_read,
            cache_write,
        }),
    }
}

#[test]
fn reconciles_fractional_charges() {
    let usage = UsageMetadata {
        prompt_token_count: 2,
        candidates_token_count: 1,
        cache_read_input_token_count: Some(1),
        ..Default::default()
    };
    assert_eq!(
        estimate(&json!({"input":0.4,"output":0.4,"cache_read":0.4}), &usage),
        Some(priced(0, 1, 0, 0))
    );
}
