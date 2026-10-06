//! Request customization and response observation at the HTTP boundary.

/// Applies provider-specific JSON fields and headers before a generation request.
///
/// The adapter must preserve the requested response mode and must not perform I/O.
/// It runs for each HTTP attempt under the model's configured retry policy.
///
/// # Example
///
/// ```ignore
/// let adapter: adk_model::openai::RequestAdapter = std::sync::Arc::new(|body, headers| {
///     body["metadata"] = serde_json::json!({"application": "example"});
///     headers.insert("x-request-source", "example".parse().unwrap());
///     Ok(())
/// });
/// let client = client.with_request_adapter(adapter);
/// ```
pub type RequestAdapter = std::sync::Arc<
    dyn Fn(
            &mut serde_json::Value,
            &mut reqwest::header::HeaderMap,
        ) -> Result<(), adk_core::AdkError>
        + Send
        + Sync,
>;

/// Observes response headers before status handling or body consumption.
///
/// The callback receives headers for every Responses generation attempt, including
/// unsuccessful statuses. It must not perform I/O or retain sensitive headers.
pub type ResponseObserver = std::sync::Arc<dyn Fn(&http::HeaderMap) + Send + Sync>;
