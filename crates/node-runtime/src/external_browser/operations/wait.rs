//! Primitive adapters adapted from adk-browser b3e360bbf49ca39d49cccba182c0f7cb4d251970.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
use adk_browser::BrowserSession;
use adk_core::Result;
use serde_json::{Value, json};
use std::time::Duration;

pub(super) async fn wait_for_element(browser: &BrowserSession, args: Value) -> Result<Value> {
    let selector = args
        .get("selector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| adk_core::AdkError::tool("Missing 'selector' parameter"))?;

    let timeout = args.get("timeout").and_then(|v| v.as_u64()).unwrap_or(30);

    let visible = args
        .get("visible")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let element = if visible {
        browser.wait_for_clickable(selector, timeout).await?
    } else {
        browser.wait_for_element(selector, timeout).await?
    };

    let text = element.text().await.unwrap_or_default();
    let text_preview = text.chars().take(100).collect::<String>();

    Ok(json!({
        "success": true,
        "found": true,
        "element_text": text_preview
    }))
}

pub(super) async fn wait(_browser: &BrowserSession, args: Value) -> Result<Value> {
    let seconds = args
        .get("seconds")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| adk_core::AdkError::tool("Missing 'seconds' parameter"))?;

    // Cap at 30 seconds
    let seconds = seconds.min(30.0);

    tokio::time::sleep(Duration::from_secs_f64(seconds)).await;

    Ok(json!({
        "success": true,
        "waited_seconds": seconds
    }))
}

pub(super) async fn wait_for_page_load(browser: &BrowserSession, args: Value) -> Result<Value> {
    let timeout = args.get("timeout").and_then(|v| v.as_u64()).unwrap_or(30);

    let script = "return document.readyState";
    let start = std::time::Instant::now();

    loop {
        let result = browser.execute_script(script).await?;
        if result.as_str() == Some("complete") {
            break;
        }

        if start.elapsed().as_secs() > timeout {
            return Err(adk_core::AdkError::tool(format!(
                "Page load timeout after {}s",
                timeout
            )));
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let url = browser.current_url().await?;
    let title = browser.title().await?;

    Ok(json!({
        "success": true,
        "url": url,
        "title": title,
        "ready_state": "complete"
    }))
}

pub(super) async fn wait_for_text(browser: &BrowserSession, args: Value) -> Result<Value> {
    let text = args
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| adk_core::AdkError::tool("Missing 'text' parameter"))?;

    let timeout = args.get("timeout").and_then(|v| v.as_u64()).unwrap_or(30);

    let escaped = adk_browser::escape_js_string(text);
    let script = format!("return document.body.innerText.includes('{escaped}')");

    let start = std::time::Instant::now();

    loop {
        let result = browser.execute_script(&script).await?;
        if result.as_bool() == Some(true) {
            return Ok(json!({
                "success": true,
                "found": true,
                "text": text
            }));
        }

        if start.elapsed().as_secs() > timeout {
            return Err(adk_core::AdkError::tool(format!(
                "Text '{}' not found after {}s",
                text, timeout
            )));
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
