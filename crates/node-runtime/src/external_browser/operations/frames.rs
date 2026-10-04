//! Primitive adapters adapted from adk-browser b3e360bbf49ca39d49cccba182c0f7cb4d251970.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
use adk_browser::BrowserSession;
use adk_core::AdkError;
use adk_core::Result;
use serde_json::{Value, json};

pub(super) async fn switch_to_frame(browser: &BrowserSession, args: Value) -> Result<Value> {
    let index = args.get("index").and_then(|v| v.as_u64());
    let selector = args.get("selector").and_then(|v| v.as_str());

    if let Some(idx) = index {
        browser.switch_to_frame_by_index(idx as u16).await?;
        Ok(json!({
            "success": true,
            "switched_to_frame": idx
        }))
    } else if let Some(sel) = selector {
        browser.switch_to_frame_by_selector(sel).await?;
        Ok(json!({
            "success": true,
            "switched_to_frame": sel
        }))
    } else {
        Err(AdkError::tool("Must provide either 'index' or 'selector'"))
    }
}

pub(super) async fn switch_to_parent_frame(
    browser: &BrowserSession,
    _args: Value,
) -> Result<Value> {
    browser.switch_to_parent_frame().await?;

    Ok(json!({
        "success": true,
        "message": "Switched to parent frame"
    }))
}

pub(super) async fn switch_to_default_content(
    browser: &BrowserSession,
    _args: Value,
) -> Result<Value> {
    browser.switch_to_default_content().await?;

    Ok(json!({
        "success": true,
        "message": "Switched to default content"
    }))
}
