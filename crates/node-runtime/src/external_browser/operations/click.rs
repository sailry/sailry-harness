//! Primitive adapters adapted from adk-browser b3e360bbf49ca39d49cccba182c0f7cb4d251970.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
use super::{Session, unavailable};
use sailry_protocol::Fault;
use serde_json::{Value, json};

pub(super) async fn click(browser: &Session, args: Value) -> Result<Value, Fault> {
    let selector = args
        .get("selector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| unavailable("Missing 'selector' parameter"))?;

    let wait_timeout = args
        .get("wait_timeout")
        .and_then(|v| v.as_u64())
        .unwrap_or(10);

    // Wait for element to be clickable, then click
    let element = browser.wait_for_clickable(selector, wait_timeout).await?;

    element
        .click()
        .await
        .map_err(|e| unavailable(format!("Click failed: {}", e)))?;

    // Get element info for response
    let tag_name = element
        .tag_name()
        .await
        .unwrap_or_else(|_| "unknown".to_string());

    let text = element.text().await.unwrap_or_default();
    let element_info = if text.is_empty() {
        tag_name
    } else {
        format!(
            "{}: {}",
            tag_name,
            text.chars().take(50).collect::<String>()
        )
    };

    // Include page context so the agent knows what happened after the click
    let context = browser.page_context().await.unwrap_or_default();

    Ok(json!({
        "success": true,
        "clicked_element": element_info,
        "page": context
    }))
}

pub(super) async fn double_click(browser: &Session, args: Value) -> Result<Value, Fault> {
    let selector = args
        .get("selector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| unavailable("Missing 'selector' parameter"))?;

    let escaped = super::super::webdriver::escape_js_string(selector);

    // Execute double-click via JS and get tag name in one call
    let result = browser
            .execute_script(&format!(
                "var el = document.querySelector('{escaped}'); if (!el) return null; el.dispatchEvent(new MouseEvent('dblclick', {{'view': window, 'bubbles': true, 'cancelable': true}})); return el.tagName.toLowerCase();"
            ))
            .await?;

    let tag_name = result.as_str().unwrap_or("unknown");
    if tag_name == "unknown" && result.is_null() {
        return Err(unavailable(format!("Element not found: {selector}")));
    }

    Ok(json!({
        "success": true,
        "double_clicked_element": tag_name
    }))
}
