use super::providers::Binding;
use sailry_protocol::{Fault, Output, Request};

/// Confirm a failed durable transport result through the original request identity.
pub(in crate::settings) async fn execute(
    binding: &Binding,
    request: Request,
) -> (Result<Output, Fault>, bool) {
    let durable = request.command.durable();
    let result = binding.client.execute(request.clone()).await;
    if durable && result.is_err() {
        match binding.client.outcome(&request).await {
            Ok(sailry_protocol::RequestOutcome::Completed(result)) => (*result, true),
            _ => (result, false),
        }
    } else {
        (result, true)
    }
}
