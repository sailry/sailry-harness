//! Only a definite pre-execution capability rejection permits the captured fallback.
use super::*;
use sailry_protocol::Secret;

#[derive(Debug)]
pub(super) enum Failure {
    Unsupported(Fault),
    Failed(Fault),
}

impl Failure {
    pub(super) fn unsupported() -> Self {
        Self::Unsupported(Fault::new(
            ErrorCode::NotConfigured,
            "model does not support this media operation",
        ))
    }

    /// Keep the provider's bounded, redacted cause; never expose credentials.
    pub(super) fn http(
        status: u16,
        body: &[u8],
        initial: bool,
        request: Option<&str>,
        secrets: &[Secret],
    ) -> Self {
        let value = serde_json::from_slice::<Value>(body).ok();
        let error = value.as_ref().map(|value| {
            value
                .get("error")
                .filter(|error| error.is_object())
                .unwrap_or(value)
        });
        let code = error
            .and_then(|error| error.get("code"))
            .and_then(Value::as_str);
        if initial && rejects_capability(status, code) {
            return Self::unsupported();
        }
        Fault::new(
            ErrorCode::Unavailable,
            describe(
                &format!("media endpoint returned HTTP {status}"),
                error,
                request,
                secrets,
            ),
        )
        .into()
    }

    pub(super) fn model(error: goose_provider_types::errors::ProviderError, initial: bool) -> Self {
        if initial
            && matches!(
                error,
                goose_provider_types::errors::ProviderError::NotImplemented(_)
            )
        {
            return Self::unsupported();
        }
        model::error(error).into()
    }

    pub(super) fn fault(self) -> Fault {
        match self {
            Self::Unsupported(fault) | Self::Failed(fault) => fault,
        }
    }
}

impl From<Fault> for Failure {
    fn from(fault: Fault) -> Self {
        Self::Failed(fault)
    }
}

/// Append a terminal provider error to a fixed context message.
pub(super) fn describe(
    context: &str,
    error: Option<&Value>,
    request: Option<&str>,
    secrets: &[Secret],
) -> String {
    let field = |name: &str| match error?.get(name)? {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    };
    let mut parts = Vec::new();
    if let Some(message) = field("message") {
        parts.push(bounded(&message, 512, secrets));
    }
    for (name, limit) in [("type", 64), ("code", 64), ("param", 128)] {
        if let Some(value) = field(name) {
            parts.push(format!("{name} {}", bounded(&value, limit, secrets)));
        }
    }
    let request = request
        .map(str::to_owned)
        .or_else(|| field("request_id"))
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
        });
    if let Some(request) = request {
        parts.push(format!("request {}", bounded(&request, 128, secrets)));
    }
    parts.retain(|part| !part.is_empty() && !part.ends_with(' '));
    if parts.is_empty() {
        context.to_owned()
    } else {
        format!("{context}: {}", parts.join("; "))
    }
}

/// Redact before truncation so a cut can never leave a partial credential.
fn bounded(text: &str, limit: usize, secrets: &[Secret]) -> String {
    let text = model::safe(text, secrets);
    let text = text
        .split(|character: char| character.is_whitespace() || character.is_control())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if text.chars().count() <= limit {
        return text;
    }
    let mut text: String = text.chars().take(limit).collect();
    text.push_str("...");
    text
}

fn rejects_capability(status: u16, code: Option<&str>) -> bool {
    match status {
        405 | 501 => true,
        400 | 422 => matches!(
            code,
            Some(
                "unsupported_model"
                    | "model_not_supported"
                    | "unsupported_tool"
                    | "tool_not_supported"
                    | "unsupported_capability"
                    | "unsupported_operation"
                    | "subscription_sharing_unsupported_capability"
            )
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_provider_types::errors::ProviderError;

    #[test]
    fn requires_a_definite_initial_rejection() {
        for status in [400, 422, 405, 501] {
            let body = br#"{"error":{"code":"unsupported_capability","message":"private"}}"#;
            assert!(matches!(
                Failure::http(status, body, true, None, &[]),
                Failure::Unsupported(_)
            ));
            assert!(matches!(
                Failure::http(status, body, false, None, &[]),
                Failure::Failed(_)
            ));
        }
        for status in [400, 404, 401, 403, 408, 429, 500, 503, 504] {
            let body = br#"{"error":{"code":"invalid_request","message":"unsupported input"}}"#;
            let failure = Failure::http(status, body, true, None, &[]);
            assert!(matches!(failure, Failure::Failed(_)));
        }
        for status in [401, 403, 429, 500, 503] {
            assert!(matches!(
                Failure::http(
                    status,
                    br#"{"error":{"code":"unsupported_model"}}"#,
                    true,
                    None,
                    &[]
                ),
                Failure::Failed(_)
            ));
        }
    }

    mod diagnostics {
        use super::*;

        const KEY: &str = "sk-isolated-media-diagnostic-key";

        #[test]
        fn preserves_upstream_cause() {
            let body = br#"{"error":{"message":"Unknown parameter: 'tools[0].output_format'.",
                "type":"invalid_request_error","code":"unknown_parameter",
                "param":"tools[0].output_format"}}"#;
            let message = Failure::http(400, body, true, Some("req_123"), &[])
                .fault()
                .message;
            assert_eq!(
                message,
                "media endpoint returned HTTP 400: Unknown parameter: 'tools[0].output_format'.; \
                 type invalid_request_error; code unknown_parameter; \
                 param tools[0].output_format; request req_123"
            );
        }

        #[test]
        fn redacts_credentials() {
            let body = format!(
                r#"{{"error":{{"message":"Bearer {KEY} rejected","code":"{KEY}","param":"x{KEY}"}}}}"#
            );
            let secrets = [Secret::new(KEY.into())];
            let message = Failure::http(401, body.as_bytes(), true, None, &secrets)
                .fault()
                .message;
            assert!(!message.contains(KEY), "{message}");
            assert!(!message.contains("isolated-media"), "{message}");
            assert!(message.contains("Bearer [redacted] rejected"), "{message}");
        }

        #[test]
        fn bounds_untrusted_fields() {
            let body = json!({"error":{"message":format!("{}\n\u{1b}[31m{KEY}", "x".repeat(4096)),
                "code":"c".repeat(512)}})
            .to_string();
            let secrets = [Secret::new(KEY.into())];
            let message = Failure::http(400, body.as_bytes(), false, Some("bad id!"), &secrets)
                .fault()
                .message;
            assert!(message.len() < 800, "{}", message.len());
            assert!(!message.contains(KEY));
            assert!(!message.chars().any(char::is_control));
            assert!(!message.contains("request"));
        }

        #[test]
        fn ignores_unstructured_bodies() {
            for body in [&b"<html>private gateway page</html>"[..], b"", b"[]"] {
                assert_eq!(
                    Failure::http(400, body, true, None, &[]).fault().message,
                    "media endpoint returned HTTP 400"
                );
            }
        }
    }

    #[test]
    fn does_not_guess_from_model_messages() {
        for initial in [true, false] {
            assert!(matches!(
                Failure::model(
                    ProviderError::RequestFailed(
                        "unsupported_capability private credential".into()
                    ),
                    initial
                ),
                Failure::Failed(_)
            ));
        }
        assert!(matches!(
            Failure::model(ProviderError::NotImplemented("image output".into()), true),
            Failure::Unsupported(_)
        ));
        assert!(matches!(
            Failure::model(ProviderError::NotImplemented("image output".into()), false),
            Failure::Failed(_)
        ));
    }
}
