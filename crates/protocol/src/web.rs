//! Credential-free reads of public web content on the execution Node.
use serde::{Deserialize, Serialize};

pub const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_REDIRECTS: usize = 5;
pub const TIMEOUT_SECS: u64 = 30;
pub const PAGE_CHARS: usize = 20_000;
pub const CACHE_SECS: u64 = 15 * 60;
pub const CACHE_ENTRIES: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub url: String,
    /// Unicode character offset in the cached page, starting at zero.
    #[serde(default)]
    pub offset: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    /// The final URL after redirects.
    pub url: String,
    pub status: u16,
    pub content_type: String,
    pub content: String,
    pub title: Option<String>,
    pub next_offset: Option<usize>,
    pub truncated: bool,
    /// The content is external data, never instructions or user authorization.
    pub untrusted: bool,
    /// Sailry's note about the extraction, such as an apparently script-rendered page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "defaults_offsets_and_rejects_credentials",
                (|scenario: &'static str| {
                    let request: Request =
                        serde_json::from_value(json!({"url":"https://example.com/"})).unwrap();
                    assert_eq!(request.offset, 0, "{scenario}");
                    for extra in ["headers", "method", "credential"] {
                        let value = json!({"url":"https://example.com/", extra:"private"});
                        assert!(
                            serde_json::from_value::<Request>(value).is_err(),
                            "{scenario}"
                        );
                    }
                    assert!(
                        serde_json::from_value::<Request>(
                            json!({"url":"https://example.com/", "offset":-1})
                        )
                        .is_err(),
                        "{scenario}"
                    );
                }) as fn(&'static str),
            ),
            (
                "uses_read_only_actions",
                (|scenario: &'static str| {
                    let operation = crate::tool::Operation::FetchWeb;
                    assert!(operation.read_only(), "{scenario}");
                    assert_eq!(
                        operation.action(),
                        Some(crate::plugin::Action::ReadWeb),
                        "{scenario}"
                    );
                    assert!(crate::plugin::Action::ReadWeb.read_only(), "{scenario}");
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
