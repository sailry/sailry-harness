//! Finite continuations share one captured invocation and ordinary durable requests.
use super::*;
use serde::Deserialize;
use std::time::Instant;
const MAX_STEPS: usize = 8;
#[derive(Deserialize)]
#[serde(untagged)]
enum Transition {
    Call(Call),
    Done(Done),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Call {
    call: Operation,
    state: Option<Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Operation {
    operation: tool::Operation,
    arguments: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Done {
    result: Value,
    output: Option<usize>,
}

fn uncertain(value: &Value) -> bool {
    value.get("isError").and_then(Value::as_bool) == Some(true)
        && serde_json::from_value::<Fault>(value["error"].clone()).is_ok_and(|fault| {
            matches!(fault.code, ErrorCode::OutcomeUnknown | ErrorCode::Cancelled)
        })
}
fn invalid(message: &str) -> Value {
    json!({"isError":true,"error":Fault::new(ErrorCode::InvalidRequest,message)})
}
impl Script {
    pub(super) async fn execute_flow(
        &self,
        context: Arc<dyn ToolContext>,
        arguments: Value,
    ) -> adk_core::Result<Value> {
        let started = Instant::now();
        let mut state = Value::Null;
        let mut outcome = Value::Null;
        let mut outputs: Vec<(Value, media::Media)> = Vec::new();
        let mut approved = false;
        let mut failed = false;
        let mut recovery: Option<usize> = None;
        for step in 0..=MAX_STEPS {
            let response=match self.evaluate(context.clone(),&self.handler.name,json!({
                "arguments":arguments,"message":self.message,"state":state,"outcome":outcome,
                "step":step,"elapsed_ms":started.elapsed().as_millis()
            })).await {
                Ok(response)=>response,
                Err(error)=> {
                    let Some((original,media))=outputs.last() else { return Err(error); };
                    if original.get("isError").and_then(Value::as_bool)==Some(true) { return Ok(original.clone()); }
                    let mut metadata=original.get("data").cloned().unwrap_or_else(|| original.clone());
                    if let Some(object)=metadata.as_object_mut() { object.insert("presentation_error".into(),error.to_string().into()); }
                    return Ok(media.restore(metadata));
                }
            };
            let transition = match serde_json::from_value::<Transition>(response) {
                Ok(value) => value,
                Err(_) => {
                    if let Some(index) = recovery {
                        return Ok(outputs[index].1.restore(outputs[index].0["data"].clone()));
                    }
                    return Ok(if failed {
                        outcome
                    } else {
                        invalid("invalid plugin flow transition")
                    });
                }
            };
            match transition {
                Transition::Done(done) => {
                    if let Some(index) = recovery {
                        return Ok(outputs[index].1.restore(done.result));
                    }
                    if failed {
                        if tool::ResultDisplay::from_value(&done.result).is_some() {
                            outcome["sailry_result"] = done.result["sailry_result"].clone();
                        }
                        if let Some(content) = done.result.get("sailry_content") {
                            outcome["sailry_content"] = content.clone();
                        }
                        return Ok(outcome);
                    }
                    return Ok(match done.output {
                        Some(index) => match outputs.get(index) {
                            Some((original, media)) if !uncertain(original) => {
                                media.restore(done.result)
                            }
                            Some((original, _)) => original.clone(),
                            None => invalid("plugin flow references an unavailable output"),
                        },
                        None => done.result,
                    });
                }
                Transition::Call(call) => {
                    if let Some(index) = recovery {
                        return Ok(outputs[index].1.restore(outputs[index].0["data"].clone()));
                    }
                    if failed {
                        return Ok(outcome);
                    }
                    if step == MAX_STEPS {
                        return Ok(invalid("plugin flow exceeds operation limit"));
                    }
                    let Some((_, operation)) = self
                        .flows
                        .iter()
                        .find(|(operation, _)| *operation == call.call.operation)
                    else {
                        return Ok(invalid("plugin flow requested an undeclared operation"));
                    };
                    // Structural validation precedes the original confirmation. Each resumed
                    // command gets its own durable identity and can never replace earlier work.
                    let result = async {
                        let mut request = operation.prepare(call.call.arguments).await?;
                        self.ingress
                            .check_plugin(self.caller, request.clone())
                            .await?;
                        if !approved && !self.is_read_only() {
                            request.id = operation
                                .authorize(context.as_ref(), arguments.clone())
                                .await?;
                            approved = true;
                        }
                        operation.dispatch(request).await
                    }
                    .await;
                    outcome = match result {
                        Ok(output) => output,
                        Err(error) => json!({"error":error,"isError":true}),
                    };
                    if uncertain(&outcome) {
                        return Ok(outcome);
                    }
                    failed = outcome.get("isError").and_then(Value::as_bool) == Some(true);
                    let media = media::Media::take(&mut outcome);
                    if call.call.operation == tool::Operation::ControlComputer && media.recovery() {
                        recovery = Some(outputs.len());
                    }
                    outputs.push((outcome.clone(), media));
                    state = call.state.unwrap_or(Value::Null);
                }
            }
        }
        unreachable!()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requires_one_explicit_transition() {
        assert!(
            serde_json::from_value::<Transition>(
                json!({"call":{"operation":"computer.read","arguments":{}},"result":{}})
            )
            .is_err()
        );
        assert!(serde_json::from_value::<Transition>(json!({"result":{},"output":0})).is_ok());
        let Transition::Call(call) = serde_json::from_value::<Transition>(
            json!({"call":{"operation":"undeclared","arguments":{}}}),
        )
        .unwrap() else {
            panic!("expected an operation transition");
        };
        assert_eq!(call.call.operation, tool::Operation::Unsupported);
        assert!(!call.call.operation.read_only());
    }
    #[test]
    fn uncertainty_never_reaches_a_continuation() {
        for code in [ErrorCode::OutcomeUnknown, ErrorCode::Cancelled] {
            assert!(uncertain(
                &json!({"error":Fault::new(code,"effects may have occurred"),"isError":true})
            ));
        }
        assert!(!uncertain(
            &json!({"error":Fault::new(ErrorCode::Unavailable,"operation unavailable"),"isError":true})
        ));
    }
}
