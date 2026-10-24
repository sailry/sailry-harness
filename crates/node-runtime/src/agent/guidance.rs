//! The ADK callback runs after the complete preceding model/tool batch settles.
//! Guidance admitted during the model phase interrupts the response instead.
use crate::store::Ingress;
use adk_core::{AdkError, BeforeModelCallback, BeforeModelResult, BeforeToolCallback};
use sailry_protocol::TurnId;
use std::sync::Arc;

pub(super) fn bind(ingress: &Arc<Ingress>, recipient: TurnId) -> BeforeModelCallback {
    let ingress = ingress.clone();
    Box::new(move |_, request| {
        let ingress = ingress.clone();
        Box::pin(async move {
            let yielded = ingress.yield_guidance(recipient).await.map_err(|error| {
                AdkError::agent("queued guidance could not be applied").with_source(error)
            })?;
            if yielded {
                return Err(AdkError::agent("turn yielded to queued guidance"));
            }
            Ok(BeforeModelResult::Continue(request))
        })
    })
}

/// Closes the interruptible model phase before any tool from the response runs.
pub(super) fn settle(ingress: &Arc<Ingress>, recipient: TurnId) -> BeforeToolCallback {
    let ingress = ingress.clone();
    Box::new(move |_| {
        let ingress = ingress.clone();
        Box::pin(async move {
            if !ingress.agents.model_settled(recipient).await {
                return Err(AdkError::agent("turn yielded to queued guidance"));
            }
            Ok(None)
        })
    })
}
