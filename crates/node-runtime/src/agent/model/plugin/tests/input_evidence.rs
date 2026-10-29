use super::*;
use crate::agent::engine::prompt::input_evidence::{Input, counts, inputs};

fn convert(input: &Input, dialect: Dialect) -> wire::Request {
    request::convert(
        &ModelConfig::new("fixture-input").with_max_tokens(Some(8192)),
        &input.system,
        &input.messages,
        &input.tools,
        request::Context {
            endpoint: "https://fixture.invalid",
            session: "fixed-fixture-session",
            effort: Effort::Default,
            features: Default::default(),
            capabilities: &capabilities(),
            dialect,
        },
    )
    .unwrap()
}

#[test]
fn projects_mirrored_results_once() {
    let (before, after) = inputs();
    for dialect in [Dialect::Generic, Dialect::OpenAi, Dialect::Gemini] {
        let original = serde_json::to_value(convert(&before, dialect)).unwrap();
        let projected = convert(&after, dialect);
        let wire = serde_json::to_value(&projected).unwrap();
        assert_eq!(original["system"], wire["system"]);
        assert_eq!(original["tools"], wire["tools"]);
        assert_eq!(
            original.to_string().matches("FILE_BODY_SENTINEL").count(),
            2
        );
        assert_eq!(wire.to_string().matches("FILE_BODY_SENTINEL").count(), 1);
        assert_eq!(
            original.to_string().matches("SKILL_BODY_SENTINEL").count(),
            7
        );
        assert_eq!(wire.to_string().matches("SKILL_BODY_SENTINEL").count(), 1);
        let mut serialized_results = 0;
        for message in &projected.messages {
            let wire::Message::Tool {
                call_id,
                name,
                parts,
                ..
            } = message
            else {
                continue;
            };
            let [wire::Part::Json { value }] = parts.as_slice() else {
                panic!("one projected tool result expected")
            };
            assert!(value.get("structuredContent").is_none());
            let text = value["content"][0]["text"].as_str().unwrap();
            if name == "read_file" {
                assert!(!text.contains("\"lines\":"));
                let reference: serde_json::Value =
                    serde_json::from_str(text.lines().last().unwrap()).unwrap();
                assert_eq!(reference["output"]["call"], call_id.as_str());
                assert!(serde_json::to_string(text).unwrap().len() <= 32 * 1024);
            } else {
                assert!(!text.contains("SKILL_BODY_SENTINEL"));
                let receipt: serde_json::Value = serde_json::from_str(text).unwrap();
                assert_eq!(receipt["output"]["call"], call_id.as_str());
            }
            // The bundled adapter stringifies this whole JSON value for its tool text.
            let text = serde_json::to_string(value).unwrap();
            serialized_results += crate::agent::engine::tokens::text(&text);
        }
        println!(
            "{}",
            json!({"scope":"synthetic_plugin_neutral","dialect":dialect,
            "note":"Offline o200k estimates, not provider usage. Segments are independently tokenized and are not additive. Serialized tool-result text is an overlapping messages subset; the package may add other request fields.",
            "before":counts(&original,&original["system"],&original["messages"]),
            "after":counts(&wire,&wire["system"],&wire["messages"]),
            "serialized_tool_result_text_tokens":serialized_results})
        );
    }
}
