//! Native operations keep artifact authority on the captured execution Node.
use super::*;

pub(super) fn handles(operation: Operation) -> bool {
    matches!(
        operation,
        Operation::WriteArtifact
            | Operation::ReadArtifact
            | Operation::ListArtifacts
            | Operation::ListArtifactRevisions
    )
}

pub(super) fn parameters(operation: Operation) -> Value {
    let (properties, required) = match operation {
        Operation::WriteArtifact => (
            json!({
                "id":{"type":["string","null"]},
                "expected_revision":{"type":["integer","null"],"minimum":1},
                "title":{"type":"string","minLength":1,"maxLength":120},
                "summary":{"type":"string","maxLength":280},
                "html":{"type":"string","minLength":1,"maxLength":sailry_protocol::artifact::MAX_HTML_BYTES}
            }),
            vec!["title", "html"],
        ),
        Operation::ReadArtifact => (
            json!({"id":{"type":"string"},"revision":{"type":["integer","null"],"minimum":1}}),
            vec!["id"],
        ),
        Operation::ListArtifacts => (
            json!({"after":{"type":["string","null"]},"limit":{"type":"integer","minimum":1,"maximum":64,"default":32}}),
            vec![],
        ),
        Operation::ListArtifactRevisions => (
            json!({"id":{"type":"string"},"before":{"type":["integer","null"],"minimum":1},"limit":{"type":"integer","minimum":1,"maximum":64,"default":32}}),
            vec!["id"],
        ),
        _ => unreachable!("artifact operation required"),
    };
    json!({"type":"object","additionalProperties":false,"properties":properties,"required":required})
}

pub(super) fn command(
    operation: Operation,
    arguments: Value,
    context: &plugin::Context,
) -> Result<Command, Fault> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Read {
        id: sailry_protocol::ArtifactId,
        revision: Option<u32>,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct List {
        after: Option<sailry_protocol::ArtifactId>,
        #[serde(default = "limit")]
        limit: u16,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Revisions {
        id: sailry_protocol::ArtifactId,
        before: Option<u32>,
        #[serde(default = "limit")]
        limit: u16,
    }
    fn limit() -> u16 {
        32
    }
    let invalid = |_| Fault::invalid("invalid artifact arguments");
    Ok(match operation {
        Operation::WriteArtifact => {
            Command::WriteArtifact(serde_json::from_value(arguments).map_err(invalid)?)
        }
        Operation::ReadArtifact => {
            let value: Read = serde_json::from_value(arguments).map_err(invalid)?;
            Command::ReadArtifact {
                id: value.id,
                revision: value.revision,
                session: context.session,
            }
        }
        Operation::ListArtifacts => {
            let value: List = serde_json::from_value(arguments).map_err(invalid)?;
            Command::ListArtifacts {
                session: context
                    .session
                    .ok_or_else(|| Fault::invalid("artifact requires a session"))?,
                after: value.after,
                limit: value.limit,
            }
        }
        Operation::ListArtifactRevisions => {
            let value: Revisions = serde_json::from_value(arguments).map_err(invalid)?;
            Command::ListArtifactRevisions {
                id: value.id,
                before: value.before,
                limit: value.limit,
                session: context.session,
            }
        }
        _ => unreachable!("artifact operation required"),
    })
}

pub(super) fn annotate(output: &sailry_protocol::Output, value: &mut Value) {
    if matches!(output, sailry_protocol::Output::Artifact(_)) {
        // Trusted rendering assets are controller implementation, not model input.
        if let Some(data) = value.get_mut("data").and_then(Value::as_object_mut) {
            data.remove("view");
        }
    }
    if let sailry_protocol::Output::ArtifactWritten { artifact, .. } = output {
        value["data"]["sailry_content"] = json!(sailry_protocol::tool::Content {
            version: 1,
            blocks: vec![sailry_protocol::tool::Block::Artifact(artifact.clone())]
        });
    }
}
