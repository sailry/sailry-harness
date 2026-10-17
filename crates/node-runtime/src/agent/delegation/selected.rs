//! Automatic role dispatch selects a tool accepting its exact captured payload.
use super::*;

struct NoRetrieval;
impl jsonschema::Retrieve for NoRetrieval {
    fn retrieve(
        &self,
        _: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Err("schema retrieval is disabled".into())
    }
}

pub(in crate::agent) fn arguments(invocation: &Invocation) -> Result<Value, Fault> {
    let role = invocation
        .message
        .selected_role()
        .map(|selected| {
            invocation
                .turn
                .roles
                .profiles
                .iter()
                .find(|role| role.reference() == selected)
                .map(|role| role.key.clone())
                .ok_or_else(|| Fault::invalid("selected role is absent from the frozen roster"))
        })
        .transpose()?;
    let task = if invocation.message.text.trim().is_empty() {
        "Complete the request using the selected context".into()
    } else {
        invocation.message.text.clone()
    };
    Ok(json!({"role":role,"task":task}))
}

pub(in crate::agent) fn accepts(schema: &Value, arguments: &Value) -> bool {
    schema["properties"].get("role").is_some()
        && schema["properties"].get("task").is_some()
        && jsonschema::draft202012::options()
            .with_retriever(NoRetrieval)
            .build(schema)
            .is_ok_and(|validator| validator.is_valid(arguments))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_automatic_payload() {
        let schema = json!({"type":"object","additionalProperties":false,"required":["task"],"properties":{"role":{"enum":["reviewer",null]},"task":{"type":"string"},"background":{"type":"boolean"}}});
        assert!(accepts(
            &schema,
            &json!({"role":"reviewer","task":"Review changes"})
        ));
        assert!(!accepts(
            &schema,
            &json!({"role":"unknown","task":"Review changes"})
        ));
        let mut restricted = schema.clone();
        restricted["required"] = json!(["task", "background"]);
        assert!(!accepts(
            &restricted,
            &json!({"role":"reviewer","task":"Review changes"})
        ));
    }

    #[test]
    fn excludes_specialized_delegation() {
        let schema = json!({"type":"object","required":["review_id","groups","record_receipt"],"properties":{"role":{"type":["string","null"]},"review_id":{"type":"string"},"groups":{"type":"array"},"record_receipt":{"type":"boolean"}}});
        assert!(!accepts(
            &schema,
            &json!({"role":"reviewer","task":"Review changes"})
        ));
        assert!(!accepts(
            &json!({}),
            &json!({"role":"reviewer","task":"Review changes"})
        ));
    }

    #[test]
    fn rejects_external_schema_retrieval() {
        let schema =
            json!({"properties":{"role":{},"task":{}},"$ref":"https://example.invalid/tool.json"});
        assert!(!accepts(
            &schema,
            &json!({"role":"reviewer","task":"Review changes"})
        ));
    }
}
