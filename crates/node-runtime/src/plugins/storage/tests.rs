use super::*;
use sailry_protocol::plugin::storage::{Collection, Scope};
use serde_json::json;

fn declaration(schema: Value) -> Declaration {
    Declaration {
        collections: vec![Collection {
            scope: Scope::Node,
            key: Some("entry".into()),
            prefix: None,
            schema,
        }],
    }
}

#[test]
fn validates_inline_constraints_and_defaults() {
    for schema in [
        json!({"type":"object","properties":{"title":{"type":"string","default":"Untitled"}}}),
        json!({"$defs":{"count":{"type":"integer","default":0}},"properties":{"count":{"$ref":"#/$defs/count"}}}),
        json!(true),
    ] {
        validate(&declaration(schema)).unwrap();
    }
    for schema in [
        json!({"additionalProperties":false}),
        json!({"properties":{"child":{"additionalProperties":false}}}),
        json!({"unevaluatedProperties":false}),
        json!({"properties":{"count":{"type":"integer","default":"invalid"}}}),
        json!({"$ref":"https://schemas.invalid/schema.json"}),
        json!({"$ref":"file:///tmp/schema.json"}),
        json!({"$ref":"#/$defs/missing"}),
        json!({"type":"invalid"}),
        json!({"default":"x".repeat(storage::MAX_SCHEMA_BYTES)}),
    ] {
        assert_eq!(
            validate(&declaration(schema)).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
    let mut nested = json!(true);
    for _ in 0..=storage::MAX_SCHEMA_DEPTH {
        nested = json!({"properties":{"child":nested}});
    }
    assert!(validate(&declaration(nested)).is_err());
}

#[test]
fn projects_nested_defaults_without_replacing_null() {
    let schema = json!({
        "$defs":{"child":{"type":"object","properties":{"ready":{"type":"boolean","default":false}}}},
        "properties":{
            "title":{"type":["string","null"],"default":"Untitled"},
            "nested":{"$ref":"#/$defs/child","default":{}},
            "items":{"type":"array","items":{"$ref":"#/$defs/child"}},
            "choice":{"oneOf":[{"properties":{"kind":{"default":"first"}}}]}
        }
    });
    validate(&declaration(schema.clone())).unwrap();
    let original = json!({"title":null,"items":[{}],"choice":{},"unknown":42});
    let mut projected = original.clone();
    project(&schema, &mut projected).unwrap();
    assert_eq!(
        projected,
        json!({"title":null,"nested":{"ready":false},"items":[{"ready":false}],"choice":{},"unknown":42})
    );
    assert_eq!(
        original,
        json!({"title":null,"items":[{}],"choice":{},"unknown":42})
    );
    let mut null = Value::Null;
    project(&schema, &mut null).unwrap();
    assert_eq!(null, Value::Null);
}

#[test]
fn preserves_unknown_objects_and_replaces_declared_fields() {
    let schema = json!({"type":"object","properties":{
        "title":{"type":"string","default":"Untitled"},
        "removed":{"type":"string"},
        "nested":{"properties":{"ready":{"type":"boolean"}}},
        "items":{"type":"array","items":{"properties":{"id":{"type":"integer"}}}},
        "map":{"type":"object","additionalProperties":{"type":"string"}}
    }});
    let old = json!({"title":"Old","removed":"remove","extra":7,"nested":{"ready":true,"legacy":"keep"},"items":[{"id":1,"legacy":9}],"map":{"remove":"old"}});
    let value = normalize(
        &schema,
        Some(&old),
        &json!({"nested":{"ready":false},"items":[{"id":2}],"map":{}}),
    )
    .unwrap();
    assert_eq!(
        value,
        json!({"title":"Untitled","extra":7,"nested":{"ready":false,"legacy":"keep"},"items":[{"id":2}],"map":{}})
    );
    assert!(normalize(&schema, Some(&old), &json!({"title":false})).is_err());
    assert_eq!(old["title"], "Old");
}

#[test]
fn bounds_default_expansion() {
    let schema = json!({"items":{"properties":{"text":{"default":"x".repeat(4096)}}}});
    let mut value = json!(vec![json!({}); 100]);
    assert_eq!(
        project(&schema, &mut value).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn preserves_unknown_fields_without_selecting_branches() {
    let schema = json!({"$defs":{"base":{"properties":{"first":{"type":"string"}}}},
        "$ref":"#/$defs/base","properties":{"second":{"type":"string"}},
        "allOf":[{"properties":{"third":{"type":"string"}}}]});
    let old = json!({"first":"old","second":"remove","third":"remove","legacy":true});
    assert_eq!(
        normalize(&schema, Some(&old), &json!({"first":"new"})).unwrap(),
        json!({"first":"new","legacy":true})
    );
}
