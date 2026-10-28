//! Captured credentials are removed from provider diagnostics before publication.
use async_trait::async_trait;
use futures::StreamExt;
use goose_provider_types::{
    base::{MessageStream, Provider},
    conversation::message::Message,
    errors::ProviderError,
    model::ModelConfig,
};
use sailry_protocol::{Fault, Secret};
use std::sync::Arc;

struct Protected {
    inner: Arc<dyn Provider>,
    secrets: Vec<Secret>,
}

pub(super) fn wrap(inner: Arc<dyn Provider>, secrets: Vec<Secret>) -> Arc<dyn Provider> {
    Arc::new(Protected { inner, secrets })
}

#[async_trait]
impl Provider for Protected {
    fn get_name(&self) -> &str {
        self.inner.get_name()
    }

    async fn stream(
        &self,
        config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[rmcp::model::Tool],
    ) -> Result<MessageStream, ProviderError> {
        let stream = self
            .inner
            .stream(config, system, messages, tools)
            .await
            .map_err(|error| redact(error, &self.secrets))?;
        let secrets = self.secrets.clone();
        Ok(Box::pin(stream.map(move |item| {
            item.map_err(|error| redact(error, &secrets))
        })))
    }
}

pub(super) fn redact(mut error: ProviderError, secrets: &[Secret]) -> ProviderError {
    if let Some(details) = error.details_mut() {
        *details = safe(details, secrets);
    }
    if let ProviderError::CreditsExhausted { top_up_url, .. } = &mut error {
        if top_up_url.as_ref().is_some_and(|value| {
            secrets
                .iter()
                .any(|secret| !secret.expose().is_empty() && value.contains(secret.expose()))
        }) {
            *top_up_url = None;
        }
    }
    error
}

pub(super) fn safe(value: &str, secrets: &[Secret]) -> String {
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

pub(super) fn fault(error: ProviderError) -> Fault {
    use sailry_protocol::{ErrorCode, FaultDetails, ModelFailure};
    let code = if matches!(error, ProviderError::NotConfigured) {
        ErrorCode::NotConfigured
    } else {
        ErrorCode::Unavailable
    };
    let reason = match error {
        ProviderError::Authentication(_) => ModelFailure::Unauthorized,
        ProviderError::RateLimitExceeded { .. } | ProviderError::CreditsExhausted { .. } => {
            ModelFailure::RateLimited
        }
        _ => ModelFailure::Request,
    };
    let mut fault = Fault::new(code, error.to_string());
    fault.details = Some(FaultDetails::ModelRequest { reason });
    fault
}

pub(super) fn provider(error: Fault) -> ProviderError {
    match error.code {
        sailry_protocol::ErrorCode::NotConfigured => ProviderError::NotConfigured,
        sailry_protocol::ErrorCode::PermissionDenied => {
            ProviderError::Authentication(error.message)
        }
        _ => ProviderError::RequestFailed(error.message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_overlapping_credentials() {
        let error = redact(
            ProviderError::Authentication("access-refresh and access".into()),
            &[Secret::new("access"), Secret::new("access-refresh")],
        );
        assert_eq!(
            error,
            ProviderError::Authentication("[redacted] and [redacted]".into())
        );
    }
}

#[cfg(test)]
mod streaming_tests {
    use super::*;

    struct Rejected;
    #[async_trait]
    impl Provider for Rejected {
        fn get_name(&self) -> &str {
            "fixture"
        }
        async fn stream(
            &self,
            _: &ModelConfig,
            _: &str,
            _: &[Message],
            _: &[rmcp::model::Tool],
        ) -> Result<MessageStream, ProviderError> {
            Ok(Box::pin(futures::stream::iter([Err(
                ProviderError::NetworkError("stream included private-access".into()),
            )])))
        }
    }

    #[tokio::test]
    async fn removes_credentials_from_deferred_errors() {
        let provider = wrap(Arc::new(Rejected), vec![Secret::new("private-access")]);
        let mut stream = provider
            .stream(&ModelConfig::new("model"), "", &[], &[])
            .await
            .unwrap();
        let error = stream.next().await.unwrap().unwrap_err();
        assert_eq!(
            error,
            ProviderError::NetworkError("stream included [redacted]".into())
        );
    }
}
