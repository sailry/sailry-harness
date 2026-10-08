//! Bounded structured provider codes, separate from unchecked error messages.
use adk_core::AdkError;

pub(crate) fn with_code(mut error: AdkError, code: Option<&str>) -> AdkError {
    if let Some(code) = code.filter(|code| {
        !code.is_empty()
            && code.len() <= 128
            && code.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
    }) {
        error.details.metadata.insert("provider_error_code".into(), serde_json::json!(code));
    }
    error
}

pub(crate) fn from_body(error: AdkError, body: &str) -> AdkError {
    if body.len() > 64 * 1024 {
        return error;
    }
    let Ok(body) = serde_json::from_str::<serde_json::Value>(body) else {
        return error;
    };
    let code = ["/error/code", "/error/status"].into_iter().find_map(|path| {
        body.pointer(path).and_then(serde_json::Value::as_str).filter(|code| {
            !code.is_empty()
                && code.len() <= 128
                && code.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        })
    });
    with_code(error, code)
}
