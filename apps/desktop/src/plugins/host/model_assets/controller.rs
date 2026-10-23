use super::*;
use sailry_protocol::{ModelId, plugin::Reference};
use std::{future::Future, pin::Pin};

#[derive(Clone)]
pub(super) enum Operation {
    Register(models::Definition),
    Install(Option<ModelId>),
}

#[derive(Clone)]
pub(super) struct Draft {
    operation: Operation,
    package: Reference,
    pub pending: bool,
    outcome: Option<serde_json::Value>,
}

type Completion = Pin<Box<dyn Future<Output = Result<HostValue, HostError>> + Send>>;

impl Host {
    pub(super) fn prepare_controller(&self, operation: Operation) -> Result<HostValue, HostError> {
        self.check()?;
        let package = self.context().package;
        let mut models = self.models.lock().map_err(lock_error)?;
        if !models.install {
            return Err(fault(sailry_protocol::Fault::new(
                sailry_protocol::ErrorCode::PermissionDenied,
                "model installation is not granted",
            )));
        }
        if models.manager.is_none() || models.runtime.is_none() {
            return Err(fault(sailry_protocol::Fault::new(
                sailry_protocol::ErrorCode::Unavailable,
                "speech installation requires a controller",
            )));
        }
        let id = RequestId::new();
        models.drafts.insert(
            id,
            Draft {
                operation,
                package,
                pending: false,
                outcome: None,
            },
        );
        Ok(HostValue::from(id.to_string()))
    }

    pub(in crate::plugins::host) fn complete_model(
        self: &Arc<Self>,
        request: RequestId,
    ) -> Result<Option<Completion>, HostError> {
        self.check()?;
        let package = self.context().package;
        let (draft, manager, runtime) = {
            let mut models = self.models.lock().map_err(lock_error)?;
            let Some(draft) = models.drafts.get(&request).cloned() else {
                return Ok(None);
            };
            if !models.install || draft.package != package {
                return Err(HostError::new("model request package is unavailable"));
            }
            if draft.pending {
                return Err(HostError::new("model request is already pending"));
            }
            if let Some(outcome) = draft.outcome {
                return Ok(Some(Box::pin(async move { encode(outcome) })));
            }
            let manager = models
                .manager
                .clone()
                .ok_or_else(|| HostError::new("speech installation requires a controller"))?;
            let runtime = models
                .runtime
                .clone()
                .ok_or_else(|| HostError::new("controller runtime is unavailable"))?;
            models
                .drafts
                .get_mut(&request)
                .expect("model request")
                .pending = true;
            (draft, manager, runtime)
        };
        let owner = self.clone();
        let feedback = if let Operation::Install(id) = draft.operation {
            let presenter = manager.clone();
            let worker = runtime.clone();
            gpui_shell::with_current_app(|cx| {
                let window = cx.active_window()?;
                window
                    .update(cx, |_, window, cx| {
                        crate::installations::external_controller(
                            presenter, worker, id, request, window, cx,
                        )
                    })
                    .ok()
            })
            .flatten()
        } else {
            None
        };
        Ok(Some(Box::pin(async move {
            let worker = runtime.clone();
            let job = runtime.spawn(async move {
                let result = match draft.operation {
                    Operation::Register(definition) => {
                        let manager = manager.clone();
                        tokio::task::spawn_blocking(move || manager.register(definition)).await
                            .map_err(|_| "dictation_storage").and_then(|result| result)
                            .map(|id| json!({"kind":"model_registered","data":{"id":id}}))
                    }
                    Operation::Install(id) => {
                        let mut changes = manager.subscribe();
                        match manager.start_request(id, None, request, &worker) {
                            Err(error) => Err(error),
                            Ok(_) => loop {
                                if let Some(result) = manager.outcome(request) {
                                    break result.map(|_| json!({"kind":"model_installed","data":{"id":id}}));
                                }
                                if changes.changed().await.is_err() { break Err("dictation_unavailable"); }
                            },
                        }
                    }
                };
                let outcome = match result {
                    Ok(output) => json!({"Ok":output}),
                    Err(error) => json!({"Err":sailry_protocol::Fault::new(
                        if error == "dictation_busy" { sailry_protocol::ErrorCode::Busy }
                        else if error == "dictation_cancelled" { sailry_protocol::ErrorCode::Cancelled }
                        else { sailry_protocol::ErrorCode::Internal }, error,
                    )}),
                };
                if let Some(feedback) = feedback {
                    let result = outcome.get("Err").cloned().map(serde_json::from_value)
                        .transpose().map_err(|_| HostError::new("invalid controller model outcome"))?;
                    let _ = feedback.send(result.map_or(Ok(()), Err));
                }
                if let Some(draft) = owner.models.lock().map_err(lock_error)?.drafts.get_mut(&request) {
                    draft.pending = false;
                    draft.outcome = Some(outcome.clone());
                }
                encode(outcome)
            });
            job.await
                .map_err(|_| HostError::new("controller model worker failed"))?
        })))
    }

    pub(in crate::plugins::host) fn controller_requests(
        &self,
        requests: &[RequestId],
    ) -> Result<Vec<RequestId>, HostError> {
        let package = self.context().package;
        let models = self.models.lock().map_err(lock_error)?;
        Ok(requests
            .iter()
            .filter(|id| {
                models.drafts.get(id).is_some_and(|draft| {
                    draft.package == package && matches!(draft.operation, Operation::Install(_))
                })
            })
            .copied()
            .collect())
    }

    pub(in crate::plugins::host) fn controller_manager(
        &self,
    ) -> Result<Arc<crate::dictation::models::Manager>, HostError> {
        self.check()?;
        self.models
            .lock()
            .map_err(lock_error)?
            .manager
            .clone()
            .ok_or_else(|| HostError::new("speech installation requires a controller"))
    }
}
