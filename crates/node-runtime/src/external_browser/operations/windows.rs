//! Primitive adapters adapted from adk-browser b3e360bbf49ca39d49cccba182c0f7cb4d251970.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
use adk_browser::BrowserSession;
use adk_core::AdkError;
use adk_core::Result;
use serde_json::{Value, json};

pub(super) async fn list_windows(browser: &BrowserSession, _args: Value) -> Result<Value> {
    let (windows, current) = browser.list_windows().await?;

    Ok(json!({
        "success": true,
        "windows": windows,
        "current_window": current,
        "count": windows.len()
    }))
}

pub(super) async fn new_tab(browser: &BrowserSession, args: Value) -> Result<Value> {
    let url = args.get("url").and_then(|v| v.as_str());

    let handle = browser.new_tab().await?;

    if let Some(url) = url {
        browser.navigate(url).await?;
    }

    let current_url = browser.current_url().await.unwrap_or_default();

    Ok(json!({
        "success": true,
        "window_handle": handle,
        "url": current_url
    }))
}

pub(super) async fn new_window(browser: &BrowserSession, args: Value) -> Result<Value> {
    let url = args.get("url").and_then(|v| v.as_str());

    let handle = browser.new_window().await?;

    if let Some(url) = url {
        browser.navigate(url).await?;
    }

    let current_url = browser.current_url().await.unwrap_or_default();

    Ok(json!({
        "success": true,
        "window_handle": handle,
        "url": current_url
    }))
}

pub(super) async fn switch_window(browser: &BrowserSession, args: Value) -> Result<Value> {
    let handle = args
        .get("handle")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AdkError::tool("Missing 'handle' parameter"))?;

    browser.switch_to_window(handle).await?;

    let url = browser.current_url().await.unwrap_or_default();
    let title = browser.title().await.unwrap_or_default();

    Ok(json!({
        "success": true,
        "switched_to": handle,
        "url": url,
        "title": title
    }))
}

pub(super) async fn close_window(browser: &BrowserSession, _args: Value) -> Result<Value> {
    browser.close_window().await?;

    Ok(json!({
        "success": true,
        "message": "Window closed"
    }))
}

pub(super) async fn maximize_window(browser: &BrowserSession, _args: Value) -> Result<Value> {
    browser.maximize_window().await?;

    Ok(json!({
        "success": true,
        "message": "Window maximized"
    }))
}

pub(super) async fn minimize_window(browser: &BrowserSession, _args: Value) -> Result<Value> {
    browser.minimize_window().await?;

    Ok(json!({
        "success": true,
        "message": "Window minimized"
    }))
}

pub(super) async fn set_window_size(browser: &BrowserSession, args: Value) -> Result<Value> {
    let width = args
        .get("width")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| AdkError::tool("Missing 'width' parameter"))? as u32;

    let height = args
        .get("height")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| AdkError::tool("Missing 'height' parameter"))? as u32;

    let x = args.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let y = args.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

    browser.set_window_rect(x, y, width, height).await?;

    Ok(json!({
        "success": true,
        "width": width,
        "height": height,
        "x": x,
        "y": y
    }))
}
