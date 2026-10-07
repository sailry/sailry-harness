//! Promote only ADK's captured preamble, never conversation or tool data.
use super::instructions::{self, Scope};
use crate::{
    agent::catalog::Registration,
    store::{Ingress, agent::Invocation},
};
use adk_core::{AdkError, BeforeModelCallback, BeforeModelResult, Content, LlmRequest, Part};
use sailry_link::CancellationToken;
use sailry_protocol::{conversation::reference::Target, tool::Operation};
use std::{collections::BTreeMap, sync::Arc};

pub(in crate::agent) fn bind(
    ingress: &Arc<Ingress>,
    invocation: &Invocation,
    instruction: String,
    tools: &[Registration],
    stop: CancellationToken,
) -> BeforeModelCallback {
    let expected = Content::new("user").with_text(instruction);
    let project = invocation.project.is_some() && invocation.connections.bound.is_none();
    let worktree = invocation.worktree;
    let initial: Vec<_> = invocation
        .message
        .references
        .iter()
        .filter_map(|reference| match &reference.target {
            Target::File(path) => Some(Scope::File(path.clone())),
            Target::Directory(path) => Some(Scope::Directory(path.clone())),
            _ => None,
        })
        .collect();
    // Only tools in the admitted catalog can establish project scopes. Do not
    // infer authority from a similarly named MCP or third-party function.
    let operations: BTreeMap<_, _> = tools
        .iter()
        .filter_map(|registration| {
            let package = invocation.plugins.iter().find(|package| {
                registration.plugin.as_deref() == Some(package.summary.name.as_str())
            })?;
            let name = registration.tool.name();
            let operation = crate::plugins::tools::operation(package, name)?;
            matches!(
                operation,
                Operation::ReadFile | Operation::ListDirectory | Operation::ReadOffice
            )
            .then(|| (name.to_owned(), operation))
        })
        .collect();
    let ingress = ingress.clone();
    Box::new(move |_, mut request| {
        let expected = expected.clone();
        let initial = initial.clone();
        let operations = operations.clone();
        let ingress = ingress.clone();
        let stop = stop.clone();
        Box::pin(async move {
            promote(&mut request, &expected)?;
            if project {
                let scopes = scopes(&request, initial, &operations);
                let guides = instructions::load(&ingress, worktree, &scopes, &stop)
                    .await
                    .map_err(|error| {
                        AdkError::agent("project instructions could not be loaded")
                            .with_source(error)
                    })?;
                if !guides.is_empty() {
                    request.contents[0].parts.push(Part::Text { text: guides });
                }
            }
            Ok(BeforeModelResult::Continue(request))
        })
    })
}

fn promote(request: &mut LlmRequest, expected: &Content) -> adk_core::Result<()> {
    let content = request
        .contents
        .first_mut()
        .ok_or_else(|| AdkError::agent("model preamble is missing"))?;
    if content.parts != expected.parts || !matches!(content.role.as_str(), "user" | "system") {
        return Err(AdkError::agent(
            "model preamble does not match the captured instructions",
        ));
    }
    content.role = "system".into();
    Ok(())
}

fn scopes(
    request: &LlmRequest,
    mut initial: Vec<Scope>,
    operations: &BTreeMap<String, Operation>,
) -> Vec<Scope> {
    let parts: Vec<_> = request
        .contents
        .iter()
        .flat_map(|content| {
            content
                .parts
                .iter()
                .map(move |part| (content.role.as_str(), part))
        })
        .collect();
    for (index, (role, part)) in parts.iter().enumerate() {
        if *role != "model" {
            continue;
        }
        let Part::FunctionCall { name, args, id, .. } = part else {
            continue;
        };
        let Some(operation) = operations.get(name) else {
            continue;
        };
        let Some(path) = args.get("path").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let result = parts[index + 1..].iter().find_map(|(role, part)| {
            let Part::FunctionResponse {
                function_response: result,
                id: result_id,
                ..
            } = part
            else {
                return None;
            };
            (*role == "function"
                && result.name == *name
                && result_id == id
                && (id.is_some() || returned_path(&result.response) == Some(path)))
            .then_some(&result.response)
        });
        let Some(result) = result.filter(|result| {
            result.get("error").is_none()
                && result.get("isError") != Some(&serde_json::Value::Bool(true))
        }) else {
            continue;
        };
        let Some(path) = returned_path(result) else {
            continue;
        };
        let scope = if *operation == Operation::ListDirectory {
            Scope::Directory(path.into())
        } else {
            Scope::File(path.into())
        };
        if !initial.contains(&scope) {
            initial.push(scope);
        }
    }
    initial
}

fn returned_path(result: &serde_json::Value) -> Option<&str> {
    // Direct host tools return the output data; scripted host operations retain
    // the current typed Output envelope. Both use the same Node-owned path.
    result
        .get("data")
        .unwrap_or(result)
        .get("path")
        .and_then(serde_json::Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(contents: Vec<Content>) -> LlmRequest {
        LlmRequest {
            model: "fixture".into(),
            contents,
            config: None,
            tools: Default::default(),
            previous_response_id: None,
        }
    }

    #[test]
    fn promotes_only_the_captured_preamble() {
        let preamble = Content::new("user").with_text("Policy");
        let task = Content::new("user").with_text("Policy");
        let result = Content::new("user").with_text("Untrusted tool instructions");
        let mut request = request(vec![preamble.clone(), task.clone(), result.clone()]);
        promote(&mut request, &preamble).unwrap();
        assert_eq!(request.contents[0].role, "system");
        assert_eq!(request.contents[1].role, task.role);
        assert_eq!(request.contents[1].parts, task.parts);
        assert_eq!(request.contents[2].role, result.role);
        assert_eq!(request.contents[2].parts, result.parts);
        promote(&mut request, &preamble).unwrap();
        request.contents[0] = Content::new("user").with_text("Unrelated task");
        assert!(promote(&mut request, &preamble).is_err());
        assert_eq!(request.contents[0].role, "user");
        assert!(promote(&mut self::request(vec![]), &preamble).is_err());
    }

    #[test]
    fn discovers_only_admitted_model_operations() {
        let call = |name: &str, path: &str| Part::FunctionCall {
            name: name.into(),
            args: json!({"path":path}),
            id: None,
            thought_signature: None,
        };
        let mut model = Content::new("model");
        model.parts = vec![
            call("files", "nested/source.rs"),
            call("list", "other"),
            call("mcp_read_file", "untrusted"),
            Part::FunctionCall {
                name: "files".into(),
                args: json!({"path":"../invalid"}),
                id: Some("failed".into()),
                thought_signature: None,
            },
        ];
        let mut user = Content::new("user");
        user.parts.push(call("files", "untrusted-user"));
        let operations = BTreeMap::from([
            ("files".into(), Operation::ReadFile),
            ("list".into(), Operation::ListDirectory),
        ]);
        let initial = vec![Scope::File("nested/source.rs".into())];
        let mut results = Content::new("function");
        for (name, path) in [
            ("files", "nested/source.rs"),
            ("list", "other"),
            ("mcp_read_file", "untrusted"),
        ] {
            results.parts.push(Part::FunctionResponse {
                function_response: adk_core::FunctionResponseData::new(
                    name,
                    json!({"path":path, "text":"Fixture"}),
                ),
                id: None,
                annotations: None,
            });
        }
        results.parts.push(Part::FunctionResponse {
            function_response: adk_core::FunctionResponseData::new(
                "files",
                json!({"error":{"message":"Invalid path"},"isError":true}),
            ),
            id: Some("failed".into()),
            annotations: None,
        });
        assert_eq!(
            scopes(&request(vec![model, user, results]), initial, &operations),
            vec![
                Scope::File("nested/source.rs".into()),
                Scope::Directory("other".into())
            ]
        );
    }

    #[test]
    fn uses_completed_paths_without_reusing_prior_results() {
        let call = |path: &str, id: &str| Content {
            role: "model".into(),
            parts: vec![Part::FunctionCall {
                name: "files".into(),
                args: json!({"path":path}),
                id: Some(id.into()),
                thought_signature: None,
            }],
        };
        let result = |value, id: &str| Content {
            role: "function".into(),
            parts: vec![Part::FunctionResponse {
                function_response: adk_core::FunctionResponseData::new("files", value),
                id: Some(id.into()),
                annotations: None,
            }],
        };
        let request = request(vec![
            call("requested/source.rs", "reused"),
            result(json!({"path":"returned/source.rs"}), "reused"),
            call("../invalid", "reused"),
            result(json!({"error":{"message":"Invalid path"}}), "reused"),
            call("requested/other.rs", "next"),
            result(
                json!({"type":"file_content","data":{"path":"other/actual.rs"}}),
                "next",
            ),
            call("pending/source.rs", "pending"),
        ]);
        assert_eq!(
            scopes(
                &request,
                vec![],
                &BTreeMap::from([("files".into(), Operation::ReadFile)])
            ),
            vec![
                Scope::File("returned/source.rs".into()),
                Scope::File("other/actual.rs".into())
            ]
        );
    }
}
