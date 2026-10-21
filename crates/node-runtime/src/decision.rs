//! Typed decision work shares the Node's local model owner and inference queue.
use crate::models::{
    availability,
    registry::Registry,
    runtime::{Engine, Lane},
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    decision::{Request, Response},
    models::Kind,
};
use std::sync::Arc;

pub(crate) async fn decide(
    registry: &Arc<Registry>,
    request: Request,
    cancel: &CancellationToken,
) -> Result<Response, Fault> {
    request.validate()?;
    availability::check(Kind::Decision).await?;
    let choice = registry.settings().choice(Kind::Decision);
    if !choice.enabled {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "local decision service is disabled",
        ));
    }
    let id = request
        .model
        .as_deref()
        .or(choice.model.as_deref())
        .ok_or_else(|| {
            Fault::new(
                ErrorCode::NotConfigured,
                "a decision model must be selected",
            )
        })?;
    let resource = registry.resource(Kind::Decision, id, Some(&choice))?;
    registry
        .execute(
            resource,
            Lane::Interactive,
            cancel,
            move |engine, cancel| {
                let Engine::Decision(engine) = engine else {
                    return Err(Fault::invalid("decision engine required"));
                };
                engine
                    .decide(&request, || cancel.is_cancelled())
                    .map_err(error)
            },
        )
        .await
}

pub(crate) fn error(error: sailry_decision::Error) -> Fault {
    match error {
        sailry_decision::Error::Invalid(message) => Fault::invalid(message),
        sailry_decision::Error::Unavailable(message) => Fault::new(ErrorCode::Unavailable, message),
        sailry_decision::Error::Cancelled => {
            Fault::new(ErrorCode::Cancelled, "decision inference was cancelled")
        }
        error => Fault::new(ErrorCode::Internal, error.to_string()),
    }
}
