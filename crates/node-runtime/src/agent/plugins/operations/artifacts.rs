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
                "kind":{"type":"string","enum":["document","preview"],"default":"document"},
                "expected_revision":{"type":["integer","null"],"minimum":1},
                "title":{"type":"string","minLength":1},
                "summary":{"type":"string"},
                "html":{"type":"string","minLength":1,"maxLength":sailry_protocol::artifact::MAX_HTML_BYTES}
            }),
            vec!["title", "html"],
        ),
        Operation::ReadArtifact => (
            json!({"id":{"type":"string"},"revision":{"type":["integer","null"],"minimum":1}}),
            vec!["id"],
        ),
        Operation::ListArtifacts => (
            json!({"after":{"type":["string","null"]},"limit":{"type":"integer","minimum":1,"maximum":9_007_199_254_740_991u64,"default":32,"description":"Requested rows; each response has an independent page budget. Continue with next_after"}}),
            vec![],
        ),
        Operation::ListArtifactRevisions => (
            json!({"id":{"type":"string"},"before":{"type":["integer","null"],"minimum":1},"limit":{"type":"integer","minimum":1,"maximum":9_007_199_254_740_991u64,"default":32,"description":"Requested rows; each response has an independent page budget. Continue with next_before"}}),
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
        limit: u64,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Revisions {
        id: sailry_protocol::ArtifactId,
        before: Option<u32>,
        #[serde(default = "limit")]
        limit: u64,
    }
    fn limit() -> u64 {
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
                limit: page_limit(value.limit)?,
            }
        }
        Operation::ListArtifactRevisions => {
            let value: Revisions = serde_json::from_value(arguments).map_err(invalid)?;
            Command::ListArtifactRevisions {
                id: value.id,
                before: value.before,
                limit: page_limit(value.limit)?,
                session: context.session,
            }
        }
        _ => unreachable!("artifact operation required"),
    })
}

fn page_limit(limit: u64) -> Result<u16, Fault> {
    if !(1..=9_007_199_254_740_991).contains(&limit) {
        return Err(Fault::invalid("limit must be a positive safe integer"));
    }
    Ok(limit.min(u64::from(sailry_protocol::artifact::MAX_PAGE_LIMIT)) as u16)
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
            blocks: vec![sailry_protocol::tool::Block::Artifact {
                artifact: artifact.clone(),
            }]
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_larger_page_requests() {
        let session = sailry_protocol::SessionId::new();
        let context = plugin::Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: plugin::Reference {
                name: "example".into(),
                digest: "a".repeat(64),
                settings_revision: 0,
            },
            worktree: None,
            session: Some(session),
        };
        for limit in [65, 100_000, 9_007_199_254_740_991u64] {
            let result =
                command(Operation::ListArtifacts, json!({"limit":limit}), &context).unwrap();
            assert_eq!(
                result,
                Command::ListArtifacts {
                    session,
                    after: None,
                    limit: sailry_protocol::artifact::MAX_PAGE_LIMIT
                }
            );
        }
        for limit in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!(null),
            json!(9_007_199_254_740_992u64),
        ] {
            assert!(command(Operation::ListArtifacts, json!({"limit":limit}), &context).is_err());
        }
    }
}
