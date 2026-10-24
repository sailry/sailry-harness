//! Finite continuations share one captured invocation and ordinary durable requests.
use super::*;
use sailry_protocol::Command;
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
fn storage_conflict(operation: tool::Operation, value: &Value) -> bool {
    operation == tool::Operation::StorageTransaction
        && value.get("isError").and_then(Value::as_bool) == Some(true)
        && serde_json::from_value::<Fault>(value["error"].clone()).is_ok_and(|fault| {
            fault.code == ErrorCode::RevisionConflict
                && fault.details == Some(sailry_protocol::FaultDetails::PluginValueConflict)
        })
}
fn failure(mut outcome: Value, result: &Value, conflict: bool) -> Value {
    let original = outcome.clone();
    if conflict && let Some(context) = result.get("context") {
        outcome["context"] = context.clone();
    }
    if tool::ResultDisplay::from_value(result).is_some() {
        outcome["sailry_result"] = result["sailry_result"].clone();
    }
    if let Some(content) = result.get("sailry_content") {
        outcome["sailry_content"] = content.clone();
    }
    if conflict
        && !serde_json::to_vec(&outcome)
            .is_ok_and(|bytes| bytes.len() <= plugin::host::MAX_DATA_BYTES)
    {
        original
    } else {
        outcome
    }
}
impl Script {
    fn record_outcome(&self, context: &dyn ToolContext, error: Option<&Fault>) {
        if let Some(timeline) = self
            .context
            .turn
            .and_then(|turn| self.ingress.agents.timeline(turn))
        {
            if let Some(error) = error {
                timeline.fail_tool(
                    context.branch(),
                    context.function_call_id(),
                    &self.name,
                    Some(error.code),
                );
            } else {
                timeline.forget_failure(context.branch(), context.function_call_id(), &self.name);
            }
        }
    }

    fn invalid(&self, context: &dyn ToolContext, message: &str) -> Value {
        let error = Fault::invalid(message);
        self.record_outcome(context, Some(&error));
        json!({"isError":true,"error":error})
    }

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
        let mut conflict = false;
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
                    if let Some(object)=metadata.as_object_mut() { object.insert("presentation_error".into(),super::super::failures::presentation(&error)); }
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
                        self.invalid(context.as_ref(), "invalid plugin flow transition")
                    });
                }
            };
            match transition {
                Transition::Done(done) => {
                    if let Some(index) = recovery {
                        return Ok(outputs[index].1.restore(done.result));
                    }
                    if failed {
                        return Ok(failure(outcome, &done.result, conflict));
                    }
                    return Ok(match done.output {
                        Some(index) => match outputs.get(index) {
                            Some((original, media)) if !uncertain(original) => {
                                media.restore(done.result)
                            }
                            Some((original, _)) => original.clone(),
                            None => self.invalid(
                                context.as_ref(),
                                "plugin flow references an unavailable output",
                            ),
                        },
                        None => done.result,
                    });
                }
                Transition::Call(call) => {
                    if let Some(index) = recovery {
                        return Ok(outputs[index].1.restore(outputs[index].0["data"].clone()));
                    }
                    if failed
                        && !(conflict && call.call.operation == tool::Operation::StorageTransaction)
                    {
                        return Ok(outcome);
                    }
                    if step == MAX_STEPS {
                        return Ok(
                            self.invalid(context.as_ref(), "plugin flow exceeds operation limit")
                        );
                    }
                    if failed {
                        // Only a known, fully rolled-back storage CAS can reach
                        // another declared storage step. Authority is rechecked below.
                        tokio::task::yield_now().await;
                        self.record_outcome(context.as_ref(), None);
                    }
                    let Some((_, operation)) = self
                        .flows
                        .iter()
                        .find(|(operation, _)| *operation == call.call.operation)
                    else {
                        return Ok(self.invalid(
                            context.as_ref(),
                            "plugin flow requested an undeclared operation",
                        ));
                    };
                    // Structural validation precedes the original confirmation. Each resumed
                    // command gets its own durable identity and can never replace earlier work.
                    let result = async {
                        let mut request = operation.prepare(call.call.arguments).await?;
                        if let Command::RunCommand { access, .. } = &mut request.command {
                            *access = self
                                .ingress
                                .approved_command_access(
                                    self.context.turn.ok_or_else(|| {
                                        Fault::invalid("command flow requires a turn")
                                    })?,
                                    adk_core::ToolConfirmationRequest {
                                        acting: None,
                                        origin: None,
                                        tool_name: self.name.clone(),
                                        function_call_id: Some(context.function_call_id().into()),
                                        args: arguments.clone(),
                                    },
                                )
                                .await?;
                        }
                        self.ingress
                            .check_plugin(self.caller, request.clone())
                            .await?;
                        if !call.call.operation.read_only()
                            && operations::connections::handles(call.call.operation)
                        {
                            self.ingress
                                .approved_connection(
                                    self.context.turn.ok_or_else(|| {
                                        Fault::invalid("connection flow requires a turn")
                                    })?,
                                    adk_core::ToolConfirmationRequest {
                                        acting: None,
                                        origin: None,
                                        tool_name: self.name.clone(),
                                        function_call_id: Some(context.function_call_id().into()),
                                        args: arguments.clone(),
                                    },
                                    request.command.clone(),
                                )
                                .await?;
                        }
                        if !approved && !self.is_read_only() {
                            request.id = operation
                                .authorize(
                                    context.as_ref(),
                                    arguments.clone(),
                                    Some(request.command.clone()),
                                )
                                .await?;
                            approved = true;
                        }
                        #[cfg(any(test, feature = "test-support"))]
                        {
                            let wait = {
                                let mut pending = self.ingress.plugins.flow_wait.lock().unwrap();
                                let position = pending.iter().position(|wait| {
                                    wait.tool.as_deref().is_none_or(|tool| tool == self.name.as_str())
                                        && wait.call.as_deref().is_none_or(|call| call == context.function_call_id())
                                });
                                position.and_then(|position| pending.remove(position)).map(|entry| entry.wait)
                            };
                            if let Some(wait) = wait {
                                let _ = wait.entered.send(request.clone());
                                tokio::select! {
                                    biased;
                                    _ = self.stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "plugin flow was cancelled")),
                                    _ = wait.release.cancelled() => {},
                                }
                            }
                        }
                        operation.dispatch(context.as_ref(), request).await
                    }
                    .await;
                    if let Err(error) = &result {
                        self.record_outcome(context.as_ref(), Some(error));
                    }
                    outcome = match result {
                        Ok(output) => output,
                        Err(error) => json!({"error":error,"isError":true}),
                    };
                    if uncertain(&outcome) {
                        return Ok(outcome);
                    }
                    failed = outcome.get("isError").and_then(Value::as_bool) == Some(true);
                    conflict = storage_conflict(call.call.operation, &outcome);
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
        assert!(
            serde_json::from_value::<Transition>(
                json!({"call":{"operation":"undeclared","arguments":{}}}),
            )
            .is_err()
        );
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
    #[test]
    fn only_storage_cas_allows_another_storage_step() {
        let mut fault = Fault::new(ErrorCode::RevisionConflict, "plugin value changed");
        let outcome = |fault: &Fault| json!({"isError":true,"error":fault});
        assert!(!storage_conflict(
            tool::Operation::StorageTransaction,
            &outcome(&fault)
        ));
        fault.details = Some(sailry_protocol::FaultDetails::PluginValueConflict);
        assert!(storage_conflict(
            tool::Operation::StorageTransaction,
            &outcome(&fault)
        ));
        assert!(!storage_conflict(
            tool::Operation::WriteFile,
            &outcome(&fault)
        ));
        for code in [
            ErrorCode::OutcomeUnknown,
            ErrorCode::Cancelled,
            ErrorCode::PermissionDenied,
            ErrorCode::Unavailable,
            ErrorCode::Internal,
        ] {
            fault.code = code;
            assert!(!storage_conflict(
                tool::Operation::StorageTransaction,
                &outcome(&fault)
            ));
        }
    }
    #[test]
    fn retains_faults_beside_bounded_context() {
        let original = json!({"isError":true,"error":Fault::new(ErrorCode::RevisionConflict,"original failure")});
        let formatted = json!({"isError":false,"error":null,"saved":true,"context":{"status":"changed","saved":false}});
        assert_eq!(failure(original.clone(), &formatted, false), original);
        let explained = failure(original.clone(), &formatted, true);
        assert_eq!(explained["error"], original["error"]);
        assert_eq!(explained["isError"], true);
        assert_eq!(explained["context"], formatted["context"]);
        assert!(explained.get("saved").is_none());
        assert_eq!(
            failure(
                original.clone(),
                &json!({"context":{"latest":"x".repeat(plugin::host::MAX_DATA_BYTES)}}),
                true
            ),
            original
        );
    }
}
