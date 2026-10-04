//! Primitive adapters adapted from adk-browser b3e360bbf49ca39d49cccba182c0f7cb4d251970.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
use adk_browser::BrowserSession;
use adk_core::Result;
use serde_json::{Value, json};

pub(super) async fn screenshot(browser: &BrowserSession, args: Value) -> Result<Value> {
    let selector = args.get("selector").and_then(Value::as_str);
    let base64_image = if let Some(selector) = selector {
        browser.screenshot_element(selector).await?
    } else {
        browser.screenshot().await?
    };
    Ok(
        json!({"success":true,"base64_image":base64_image,"saved_to_artifacts":false,"artifact_name":null}),
    )
}
