use super::*;
use serde_json::json;

#[test]
fn preserves_unrelated_content() {
    let mut body = json!({
        "kind": "message",
        "id": "message-id",
        "content": [
            {"type":"text", "text":"goose.executable"},
            {
                "type":"tool_request",
                "id":"call-id",
                "arguments":{"goose.executable":false},
                "provider":{"goose.executable":"signed-provider-value"},
                "tool_meta":{
                    "goose.executable":true,
                    "goose.external_dispatch":false,
                    "title":"Read",
                    "foreign":{"goose.executable":"unchanged"}
                }
            },
            {
                "type":"tool_response",
                "tool_meta":{"goose.executable":"unchanged"},
                "result":{"value":{"goose.external_dispatch":true}}
            }
        ],
        "metadata":{"goose.executable":"unchanged"}
    });
    let mut expected = body.clone();
    expected["content"][1]["tool_meta"] = json!({
        EXECUTABLE_KEY:true,
        EXTERNAL_DISPATCH_KEY:false,
        "title":"Read",
        "foreign":{"goose.executable":"unchanged"}
    });
    assert!(record(&mut body).unwrap());
    assert_eq!(body, expected);
    assert!(!record(&mut body).unwrap());
}

#[test]
fn converts_summaries_and_operations() {
    let mut summary = json!({
        "kind":"compaction",
        "boundary":"preserved-boundary",
        "summary":{"content":[{
            "type":"tool_request",
            "tool_meta":{"goose.external_dispatch":true}
        }]}
    });
    assert!(record(&mut summary).unwrap());
    assert_eq!(
        summary["summary"]["content"][0]["tool_meta"],
        json!({EXTERNAL_DISPATCH_KEY:true})
    );
    assert_eq!(summary["boundary"], "preserved-boundary");
    let mut operation = json!({
        "kind":"operation",
        "operation":"tool_meta",
        "call":"call-id",
        "patch":{"goose.executable":false, "title":"Retained"}
    });
    assert!(record(&mut operation).unwrap());
    assert_eq!(
        operation["patch"],
        json!({EXECUTABLE_KEY:false, "title":"Retained"})
    );
    assert_eq!(operation["call"], "call-id");
    assert!(!record(&mut operation).unwrap());
}

#[test]
fn deduplicates_equal_names() {
    for (previous, current) in [
        ("goose.executable", EXECUTABLE_KEY),
        ("goose.external_dispatch", EXTERNAL_DISPATCH_KEY),
    ] {
        let mut meta = json!({previous:false, current:false});
        assert!(fields(&mut meta).unwrap());
        assert_eq!(meta, json!({current:false}));
    }
}

#[test]
fn rejects_conflicting_names() {
    for (previous, current) in [
        ("goose.executable", EXECUTABLE_KEY),
        ("goose.external_dispatch", EXTERNAL_DISPATCH_KEY),
    ] {
        let mut meta = json!({previous:true, current:false});
        let error = fields(&mut meta).unwrap_err().to_string();
        assert!(error.contains("conflicting tool execution metadata"));
    }
}

#[test]
fn keeps_existing_value_types() {
    for value in [Value::Null, json!("false"), json!({"private":"value"})] {
        let mut meta = json!({"goose.executable":value});
        assert!(fields(&mut meta).unwrap());
        assert_eq!(meta, json!({EXECUTABLE_KEY:value}));
    }
}

#[test]
fn ignores_unrelated_fields() {
    for mut body in [
        json!({"kind":"notice","patch":{"goose.executable":true}}),
        json!({"kind":"operation","operation":"note","patch":{"goose.executable":true}}),
        json!({"kind":"message","content":[{"type":"tool_request","tool_meta":null}]}),
        json!({"kind":"message","content":[{"type":"tool_request","tool_meta":"goose.executable"}]}),
    ] {
        let original = body.clone();
        assert!(!record(&mut body).unwrap());
        assert_eq!(body, original);
    }
}
