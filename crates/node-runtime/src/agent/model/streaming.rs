//! SSE negotiation follows the Node's existing transport selection.
use adk_model::openai::{RequestAdapter, WsOptions};
use reqwest::header::{ACCEPT, CACHE_CONTROL, HeaderValue};
use std::sync::Arc;

pub(super) fn adapter(
    adapter: Option<RequestAdapter>,
    websocket: Option<&WsOptions>,
) -> RequestAdapter {
    let enabled = websocket.map(|options| options.enabled.clone());
    Arc::new(move |body, headers| {
        let http = enabled.as_ref().is_none_or(|enabled| !enabled()) || body["background"] == true;
        if body["stream"] == true && http {
            headers
                .entry(ACCEPT)
                .or_insert(HeaderValue::from_static("text/event-stream"));
            headers
                .entry(CACHE_CONTROL)
                .or_insert(HeaderValue::from_static("no-cache"));
        }
        if let Some(adapter) = &adapter {
            adapter(body, headers)?;
        }
        Ok(())
    })
}

#[cfg(test)]
#[path = "streaming/tests.rs"]
mod tests;
