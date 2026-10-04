//! Primitive adapters adapted from adk-browser b3e360bbf49ca39d49cccba182c0f7cb4d251970.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
use adk_browser::BrowserSession;
use adk_core::Result;
use serde_json::{Value, json};

pub(super) async fn navigate(browser: &BrowserSession, args: Value) -> Result<Value> {
    let url = args
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| adk_core::AdkError::tool("Missing 'url' parameter"))?;

    // Validate URL
    url::Url::parse(url)
        .map_err(|e| adk_core::AdkError::tool(format!("Invalid URL '{}': {}", url, e)))?;

    // Navigate
    browser.navigate(url).await?;

    // Get result info
    let current_url = browser.current_url().await.unwrap_or_default();
    let title = browser.title().await.unwrap_or_default();

    // Include page context like interaction tools do
    match browser.page_context().await {
        Ok(page) => Ok(json!({
            "success": true,
            "url": current_url,
            "title": title,
            "page": page
        })),
        Err(e) => Ok(json!({
            "success": true,
            "url": current_url,
            "title": title,
            "page_context_error": e.to_string()
        })),
    }
}

pub(super) async fn back(browser: &BrowserSession, _args: Value) -> Result<Value> {
    browser.back().await?;

    let url = browser.current_url().await.unwrap_or_default();
    let title = browser.title().await.unwrap_or_default();

    // Include page context like interaction tools do
    match browser.page_context().await {
        Ok(page) => Ok(json!({
            "success": true,
            "url": url,
            "title": title,
            "page": page
        })),
        Err(e) => Ok(json!({
            "success": true,
            "url": url,
            "title": title,
            "page_context_error": e.to_string()
        })),
    }
}

pub(super) async fn forward(browser: &BrowserSession, _args: Value) -> Result<Value> {
    browser.forward().await?;

    let url = browser.current_url().await.unwrap_or_default();
    let title = browser.title().await.unwrap_or_default();

    // Include page context like interaction tools do
    match browser.page_context().await {
        Ok(page) => Ok(json!({
            "success": true,
            "url": url,
            "title": title,
            "page": page
        })),
        Err(e) => Ok(json!({
            "success": true,
            "url": url,
            "title": title,
            "page_context_error": e.to_string()
        })),
    }
}

pub(super) async fn refresh(browser: &BrowserSession, _args: Value) -> Result<Value> {
    browser.refresh().await?;

    let url = browser.current_url().await.unwrap_or_default();
    let title = browser.title().await.unwrap_or_default();

    // Include page context like interaction tools do
    match browser.page_context().await {
        Ok(page) => Ok(json!({
            "success": true,
            "url": url,
            "title": title,
            "page": page
        })),
        Err(e) => Ok(json!({
            "success": true,
            "url": url,
            "title": title,
            "page_context_error": e.to_string()
        })),
    }
}
