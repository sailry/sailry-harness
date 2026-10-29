use super::*;
use serde_json::json;

#[test]
fn prices_token_categories_and_tiers() {
    let cost = json!({"input":2,"output":8,"cache_read":0.5,"cache_write":3,"reasoning":10,
        "tiers":[{"tier":{"type":"context","size":100},"input":4,"output":16,"cache_read":1,"cache_write":6,"reasoning":20}]});
    let mut usage = StoredUsage {
        input: Some(100),
        output: Some(50),
        cache_read: Some(20),
        cache_write: Some(10),
        reasoning: Some(5),
        ..Default::default()
    };
    assert_eq!(estimate(&cost, &usage), Some(priced(140, 410, 10, 30)));
    usage.input = Some(101);
    assert_eq!(estimate(&cost, &usage), Some(priced(284, 820, 20, 60)));
    usage.input = Some(0);
    assert_eq!(estimate(&cost, &usage), None);
}

#[test]
fn distinguishes_unknown_and_free_prices() {
    let usage = StoredUsage {
        input: Some(100),
        output: Some(50),
        cache_read: Some(20),
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
    assert_eq!(estimate(&json!({}), &StoredUsage::default()), None);
    assert_eq!(micros(f64::INFINITY), None);
    assert_eq!(micros(-0.1), None);
    assert_eq!(micros(0.1234567), Some(123457));
}

#[test]
fn keeps_missing_counts_unpriced() {
    let cost = json!({"input":0,"output":0});
    let mut usage = StoredUsage {
        output: Some(0),
        ..Default::default()
    };
    assert_eq!(estimate(&cost, &usage), None);
    usage.input = Some(0);
    assert_eq!(estimate(&cost, &usage), Some(priced(0, 0, 0, 0)));
    usage.output = None;
    assert_eq!(estimate(&cost, &usage), None);
}

#[test]
fn leaves_audio_rates_unpriced() {
    let usage = StoredUsage {
        input: Some(100),
        output: Some(50),
        ..Default::default()
    };
    for cost in [
        json!({"input":2,"output":8,"input_audio":20}),
        json!({"input":2,"output":8,"output_audio":40}),
        json!({"input":2,"output":8,"tiers":[{"tier":{"type":"context","size":50},"input":2,"output":8,"input_audio":20}]}),
    ] {
        assert_eq!(estimate(&cost, &usage), None);
    }
}

#[test]
fn maps_captured_model_apis() {
    for (api, expected) in [
        (Some("responses"), Some("openai")),
        (Some("chat_completions"), Some("openai")),
        (Some("anthropic"), Some("anthropic")),
        (Some("gemini"), Some("google")),
        (Some("unknown"), None),
        (None, None),
    ] {
        assert_eq!(supplier(api), expected);
    }
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
    let usage = StoredUsage {
        input: Some(2),
        output: Some(1),
        cache_read: Some(1),
        ..Default::default()
    };
    assert_eq!(
        estimate(&json!({"input":0.4,"output":0.4,"cache_read":0.4}), &usage),
        Some(priced(0, 1, 0, 0))
    );
}
