//! Primitive adapters adapted from adk-browser b3e360bbf49ca39d49cccba182c0f7cb4d251970.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
use adk_browser::BrowserSession;
use adk_core::AdkError;
use adk_core::Result;
use serde_json::{Value, json};

pub(super) async fn drag_and_drop(browser: &BrowserSession, args: Value) -> Result<Value> {
    let source = args
        .get("source_selector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AdkError::tool("Missing 'source_selector' parameter"))?;

    let target = args
        .get("target_selector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AdkError::tool("Missing 'target_selector' parameter"))?;

    browser.drag_and_drop(source, target).await?;

    Ok(json!({
        "success": true,
        "dragged_from": source,
        "dropped_on": target
    }))
}

pub(super) async fn right_click(browser: &BrowserSession, args: Value) -> Result<Value> {
    let selector = args
        .get("selector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AdkError::tool("Missing 'selector' parameter"))?;

    browser.right_click(selector).await?;

    Ok(json!({
        "success": true,
        "right_clicked": selector
    }))
}

pub(super) async fn focus(browser: &BrowserSession, args: Value) -> Result<Value> {
    let selector = args
        .get("selector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AdkError::tool("Missing 'selector' parameter"))?;

    browser.focus_element(selector).await?;

    Ok(json!({
        "success": true,
        "focused": selector
    }))
}

pub(super) async fn element_state(browser: &BrowserSession, args: Value) -> Result<Value> {
    let selector = args
        .get("selector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AdkError::tool("Missing 'selector' parameter"))?;

    let state = browser.get_element_state(selector).await?;

    Ok(json!({
        "success": true,
        "selector": selector,
        "is_displayed": state.is_displayed,
        "is_enabled": state.is_enabled,
        "is_selected": state.is_selected,
        "is_clickable": state.is_clickable
    }))
}

pub(super) async fn press_key(browser: &BrowserSession, args: Value) -> Result<Value> {
    let key = args
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AdkError::tool("Missing 'key' parameter"))?;

    let selector = args.get("selector").and_then(|v| v.as_str());
    let modifiers: Vec<&str> = args
        .get("modifiers")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    browser.press_key(key, selector, &modifiers).await?;

    Ok(json!({
        "success": true,
        "key_pressed": key,
        "modifiers": modifiers,
        "target": selector
    }))
}

pub(super) async fn file_upload(browser: &BrowserSession, args: Value) -> Result<Value> {
    let selector = args
        .get("selector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AdkError::tool("Missing 'selector' parameter"))?;

    let file_path = args
        .get("file_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AdkError::tool("Missing 'file_path' parameter"))?;

    browser.upload_file(selector, file_path).await?;

    Ok(json!({
        "success": true,
        "uploaded_file": file_path,
        "to_element": selector
    }))
}

pub(super) async fn print_to_pdf(browser: &BrowserSession, args: Value) -> Result<Value> {
    let landscape = args
        .get("landscape")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let scale = args.get("scale").and_then(|v| v.as_f64()).unwrap_or(1.0);

    let pdf_base64 = browser.print_to_pdf(landscape, scale).await?;

    Ok(json!({
        "success": true,
        "pdf_base64": pdf_base64
    }))
}
