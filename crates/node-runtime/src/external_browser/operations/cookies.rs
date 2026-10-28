//! Primitive adapters adapted from adk-browser b3e360bbf49ca39d49cccba182c0f7cb4d251970.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
use super::{Session, unavailable};
use sailry_protocol::Fault;
use serde_json::{Value, json};

pub(super) async fn get_cookies(browser: &Session, _args: Value) -> Result<Value, Fault> {
    let cookies = browser.get_all_cookies().await?;
    Ok(json!({
        "success": true,
        "cookies": cookies,
        "count": cookies.len()
    }))
}

pub(super) async fn get_cookie(browser: &Session, args: Value) -> Result<Value, Fault> {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| unavailable("Missing 'name' parameter"))?;

    let cookie = browser.get_cookie(name).await?;
    Ok(json!({
        "success": true,
        "cookie": cookie
    }))
}

pub(super) async fn add_cookie(browser: &Session, args: Value) -> Result<Value, Fault> {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| unavailable("Missing 'name' parameter"))?;

    let value = args
        .get("value")
        .and_then(|v| v.as_str())
        .ok_or_else(|| unavailable("Missing 'value' parameter"))?;

    let domain = args.get("domain").and_then(|v| v.as_str());
    let path = args.get("path").and_then(|v| v.as_str());
    let secure = args.get("secure").and_then(|v| v.as_bool());
    let expiry = args.get("expiry").and_then(|v| v.as_i64());

    browser
        .add_cookie(name, value, domain, path, secure, expiry)
        .await?;

    Ok(json!({
        "success": true,
        "added_cookie": name
    }))
}

pub(super) async fn delete_cookie(browser: &Session, args: Value) -> Result<Value, Fault> {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| unavailable("Missing 'name' parameter"))?;

    browser.delete_cookie(name).await?;

    Ok(json!({
        "success": true,
        "deleted_cookie": name
    }))
}

pub(super) async fn delete_all_cookies(browser: &Session, _args: Value) -> Result<Value, Fault> {
    browser.delete_all_cookies().await?;

    Ok(json!({
        "success": true,
        "message": "All cookies deleted"
    }))
}
