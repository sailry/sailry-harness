use super::*;
use core::prelude::v1::test;
use sailry_protocol::plugin;
use serde_json::json;

#[test]
fn includes_raw_choices_and_document_identity() {
    rust_i18n::set_locale("en");
    let content = Content {
        artifact: Summary {
            kind: sailry_protocol::artifact::Kind::Document,
            id: ArtifactId::new(),
            session: sailry_protocol::SessionId::new(),
            turn: sailry_protocol::TurnId::new(),
            package: plugin::Reference {
                name: "fixture".into(),
                digest: "frozen".into(),
                settings_revision: 0,
            },
            title: "Architecture".into(),
            summary: String::new(),
            revision: 3,
            size: 10,
            created_at: 1,
            updated_at: 1,
        },
        latest_revision: 3,
        html: "<p>Plan</p>".into(),
        warnings: vec![],
        view: None,
    };
    let event = json!({"form":"choices","fields":[{"name":"storage","label":"Storage","values":[{"value":"execution-node","label":"Node"}]}]});
    let message = surface::form_message(&content, &event).unwrap();
    assert!(message.contains(&content.artifact.id.to_string()));
    assert!(message.contains("Architecture"));
    assert!(message.contains("Node (execution-node)"));
    let mut invalid = event.clone();
    invalid["fields"]
        .as_array_mut()
        .unwrap()
        .push(event["fields"][0].clone());
    assert!(surface::form_message(&content, &invalid).is_err());
}
