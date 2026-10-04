use sailry_protocol::{Command, ErrorCode, Fault, Output, Request, RequestOutcome, VERSION};

use crate::Client;

#[cfg(test)]
mod tests;

impl Client {
    /// Queries an existing durable request without admitting or replaying its command.
    /// Retain the original request across reconnects; transport failure is not NotAdmitted.
    pub async fn outcome(&self, request: &Request) -> Result<RequestOutcome, Fault> {
        if request.target != self.target() || request.version != VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        if !request.command.durable() {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "only durable commands have a stored outcome",
            ));
        }
        // Stream directly into the digest without retaining another plaintext request copy.
        let mut hasher = blake3::Hasher::new();
        serde_json::to_writer(&mut hasher, request)
            .map_err(|_| Fault::new(ErrorCode::InvalidRequest, "request could not be encoded"))?;
        let query = self.prepare(Command::InspectRequest {
            id: request.id,
            digest: hasher.finalize().to_hex().to_string(),
        });
        match self.execute(query).await? {
            Output::RequestOutcome { id, outcome } if id == request.id => Ok(outcome),
            _ => Err(Fault::new(
                ErrorCode::Internal,
                "request outcome response does not match the query",
            )),
        }
    }
}
