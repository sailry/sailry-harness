use super::types::{Error, ErrorKind, Model, ModelStream, Request};
use async_trait::async_trait;
use futures::StreamExt;
use sailry_protocol::{Fault, Secret};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

struct Protected {
    inner: Arc<dyn Model>,
    secrets: Vec<Secret>,
}

pub(super) fn wrap(inner: Arc<dyn Model>, secrets: Vec<Secret>) -> Arc<dyn Model> {
    Arc::new(Protected { inner, secrets })
}

#[async_trait]
impl Model for Protected {
    async fn call(
        &self,
        request: Request,
        cancel: CancellationToken,
    ) -> Result<ModelStream, Error> {
        let stream = self
            .inner
            .call(request, cancel)
            .await
            .map_err(|error| redact(error, &self.secrets))?;
        let secrets = self.secrets.clone();
        Ok(Box::pin(stream.map(move |item| {
            item.map_err(|error| redact(error, &secrets))
        })))
    }
}

pub(super) fn redact(mut error: Error, secrets: &[Secret]) -> Error {
    error.message = safe(&error.message, secrets);
    error
}

pub(in crate::agent) fn safe(value: &str, secrets: &[Secret]) -> String {
    let mut values: Vec<_> = secrets
        .iter()
        .map(Secret::expose)
        .filter(|value| !value.is_empty())
        .collect();
    values.sort_by_key(|value| std::cmp::Reverse(value.len()));
    let mut value = value.to_owned();
    for secret in values {
        value = value.replace(secret, "[redacted]");
    }
    value
}

pub(super) fn network(error: &Error) -> bool {
    error.kind == ErrorKind::Network
}

pub(in crate::agent) fn fault(error: Error) -> Fault {
    use sailry_protocol::{ErrorCode, FaultDetails, ModelFailure};
    let code = if error.kind == ErrorKind::NotConfigured {
        ErrorCode::NotConfigured
    } else {
        ErrorCode::Unavailable
    };
    let reason = match error.kind {
        ErrorKind::Authentication => ModelFailure::Unauthorized,
        ErrorKind::RateLimited | ErrorKind::Quota => ModelFailure::RateLimited,
        _ => ModelFailure::Request,
    };
    let mut fault = Fault::new(code, error.message);
    fault.details = Some(FaultDetails::ModelRequest { reason });
    fault
}

pub(super) fn provider(error: Fault) -> Error {
    use sailry_protocol::{ErrorCode, FaultDetails};
    let kind = match error.code {
        ErrorCode::Unavailable if matches!(error.details, Some(FaultDetails::Network)) => {
            ErrorKind::Network
        }
        ErrorCode::Cancelled => ErrorKind::Cancelled,
        ErrorCode::InvalidRequest => ErrorKind::InvalidRequest,
        ErrorCode::NotConfigured => ErrorKind::NotConfigured,
        ErrorCode::PermissionDenied => ErrorKind::Authentication,
        _ => ErrorKind::Other,
    };
    Error::new(kind, error.message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_transport_from_decode_failures() {
        assert!(network(&Error::new(
            ErrorKind::Network,
            "connection failed"
        )));
        for kind in [
            ErrorKind::InvalidResponse,
            ErrorKind::Other,
            ErrorKind::Unavailable,
        ] {
            assert!(!network(&Error::new(kind, "network unavailable")));
        }
    }

    #[test]
    fn redacts_overlapping_credentials() {
        let error = redact(
            Error::new(ErrorKind::Authentication, "access-refresh and access"),
            &[
                Secret::new("access".into()),
                Secret::new("access-refresh".into()),
            ],
        );
        assert_eq!(error.message, "[redacted] and [redacted]");
        assert_eq!(error.kind, ErrorKind::Authentication);
    }

    #[test]
    fn keeps_local_failures_nonretryable() {
        for code in [
            sailry_protocol::ErrorCode::Cancelled,
            sailry_protocol::ErrorCode::InvalidRequest,
        ] {
            assert!(!provider(Fault::new(code, "request was rejected")).retryable());
        }
    }

    #[tokio::test]
    async fn preserves_rate_limit_through_history() {
        use crate::agent::model::{Content, Message, testing};
        use sailry_protocol::{ErrorCode, FaultDetails, ModelFailure};

        let message = Message::assistant()
            .with_generated_id()
            .with_error(ErrorKind::RateLimited, "Request rate exceeded");
        assert_eq!(message.error_kind(), Some(ErrorKind::RateLimited));
        let restored = testing::roundtrip(&message).await;
        assert_eq!(restored, message);
        let [Content::Error { code, message }] = restored.content.as_slice() else {
            panic!("provider failure expected");
        };
        assert_eq!(code.as_deref(), Some("rate_limited"));
        let failure = fault(Error::new(restored.error_kind().unwrap(), message));
        assert_eq!(failure.code, ErrorCode::Unavailable);
        assert_eq!(failure.message, "Request rate exceeded");
        assert_eq!(
            failure.details,
            Some(FaultDetails::ModelRequest {
                reason: ModelFailure::RateLimited,
            })
        );
    }

    struct Rejected;

    #[async_trait]
    impl Model for Rejected {
        async fn call(&self, _: Request, _: CancellationToken) -> Result<ModelStream, Error> {
            Ok(Box::pin(futures::stream::iter([Err(Error::new(
                ErrorKind::Network,
                "stream included private-access",
            ))])))
        }
    }

    #[tokio::test]
    async fn removes_credentials_from_deferred_errors() {
        let provider = wrap(
            Arc::new(Rejected),
            vec![Secret::new("private-access".into())],
        );
        let mut stream = provider
            .call(Request::default(), CancellationToken::new())
            .await
            .unwrap();
        let error = stream.next().await.unwrap().unwrap_err();
        assert_eq!(
            error,
            Error::new(ErrorKind::Network, "stream included [redacted]")
        );
    }
}
