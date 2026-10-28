//! Primitive adapters adapted from adk-browser b3e360bbf49ca39d49cccba182c0f7cb4d251970.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
use super::{Session, unavailable};
use sailry_protocol::Fault;
use serde_json::{Value, json};

pub(super) async fn navigate(browser: &Session, args: Value) -> Result<Value, Fault> {
    let url = args
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| unavailable("Missing 'url' parameter"))?;

    // Validate URL
    url::Url::parse(url).map_err(|e| unavailable(format!("Invalid URL '{}': {}", url, e)))?;

    // Navigate
    browser.navigate(url).await?;

    context(browser).await
}

pub(super) async fn back(browser: &Session, _args: Value) -> Result<Value, Fault> {
    browser.back().await?;

    context(browser).await
}

pub(super) async fn forward(browser: &Session, _args: Value) -> Result<Value, Fault> {
    browser.forward().await?;

    context(browser).await
}

pub(super) async fn refresh(browser: &Session, _args: Value) -> Result<Value, Fault> {
    browser.refresh().await?;

    context(browser).await
}

async fn context(browser: &Session) -> Result<Value, Fault> {
    let url = browser.current_url().await.unwrap_or_default();
    let title = browser.title().await.unwrap_or_default();
    match browser.page_context().await {
        Ok(page) => Ok(json!({"success":true, "url":url, "title":title, "page":page})),
        Err(error) => Ok(
            json!({"success":true, "url":url, "title":title, "page_context_error":error.to_string()}),
        ),
    }
}
