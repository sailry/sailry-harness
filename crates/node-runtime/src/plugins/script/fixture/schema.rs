//! Offline raw-schema validation for package-owned parameter matrices.
use sailry_protocol::Fault;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub schema: Value,
    pub values: Vec<Value>,
}

#[derive(Serialize)]
pub struct Report {
    pub valid: Vec<bool>,
}

struct NoRetrieval;

impl jsonschema::Retrieve for NoRetrieval {
    fn retrieve(
        &self,
        _: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Err("fixture schema retrieval is disabled".into())
    }
}

/// Checks the original schema, not a model provider's normalized declaration.
pub fn validate(input: Input) -> Result<Report, Fault> {
    let validator = jsonschema::draft202012::options()
        .with_retriever(NoRetrieval)
        .build(&input.schema)
        .map_err(|_| Fault::invalid("invalid or unresolved fixture schema"))?;
    Ok(Report {
        valid: input
            .values
            .iter()
            .map(|value| validator.is_valid(value))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn distinguishes_invalid_values_from_schemas() {
        let report = validate(Input {
            schema: json!({"type":"integer"}),
            values: vec![json!(1), json!("secret fixture")],
        })
        .unwrap();
        assert_eq!(report.valid, [true, false]);
        let error = validate(Input {
            schema: json!({"type":"secret invalid type"}),
            values: vec![],
        })
        .err()
        .unwrap();
        assert!(!error.message.contains("secret"));
    }

    #[test]
    fn rejects_external_references() {
        for reference in ["https://example.invalid/schema", "file:///private/schema"] {
            assert!(
                validate(Input {
                    schema: json!({"$ref":reference}),
                    values: vec![],
                })
                .is_err()
            );
        }
    }

    #[test]
    fn resolves_local_references_and_conditions() {
        let report = validate(Input {
            schema: json!({"$defs":{"value":{"type":"integer"}},"type":"object","properties":{"action":{"type":"string"},"value":{"$ref":"#/$defs/value"}},"allOf":[{"if":{"properties":{"action":{"const":"write"}},"required":["action"]},"then":{"required":["value"]}}]}),
            values: vec![json!({"action":"read"}),json!({"action":"write"}),json!({"action":"write","value":1})],
        }).unwrap();
        assert_eq!(report.valid, [true, false, true]);
    }
}
