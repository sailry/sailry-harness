//! Primitive adapters adapted from adk-browser b3e360bbf49ca39d49cccba182c0f7cb4d251970.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
use adk_browser::BrowserSession;
use adk_core::Result;
use serde_json::{Value, json};

pub(super) async fn script(browser: &BrowserSession, args: Value) -> Result<Value> {
    let script = args
        .get("script")
        .and_then(|v| v.as_str())
        .ok_or_else(|| adk_core::AdkError::tool("Missing 'script' parameter"))?;

    let is_async = args.get("async").and_then(|v| v.as_bool()).unwrap_or(false);

    let result = if is_async {
        browser.execute_async_script(script).await?
    } else {
        browser.execute_script(script).await?
    };

    Ok(json!({
        "success": true,
        "result": result
    }))
}

pub(super) async fn scroll(browser: &BrowserSession, args: Value) -> Result<Value> {
    let direction = args.get("direction").and_then(|v| v.as_str());
    let selector = args.get("selector").and_then(|v| v.as_str());
    let amount = args.get("amount").and_then(|v| v.as_i64()).unwrap_or(500);

    if let Some(sel) = selector {
        // Scroll element into view
        let escaped = adk_browser::escape_js_string(sel);
        let script = format!(
            "document.querySelector('{escaped}').scrollIntoView({{ behavior: 'smooth', block: 'center' }})"
        );
        browser.execute_script(&script).await?;

        return Ok(json!({
            "success": true,
            "scrolled_to": sel
        }));
    }

    if let Some(dir) = direction {
        let script = match dir {
            "up" => format!("window.scrollBy(0, -{amount})"),
            "down" => format!("window.scrollBy(0, {amount})"),
            "top" => "window.scrollTo(0, 0)".to_string(),
            "bottom" => "window.scrollTo(0, document.body.scrollHeight)".to_string(),
            _ => {
                return Err(adk_core::AdkError::tool(format!(
                    "Invalid direction: {dir}"
                )));
            }
        };

        browser.execute_script(&script).await?;

        return Ok(json!({
            "success": true,
            "scrolled": dir
        }));
    }

    Err(adk_core::AdkError::tool(
        "Must specify either 'direction' or 'selector'",
    ))
}

pub(super) async fn hover(browser: &BrowserSession, args: Value) -> Result<Value> {
    let selector = args
        .get("selector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| adk_core::AdkError::tool("Missing 'selector' parameter"))?;

    let escaped = adk_browser::escape_js_string(selector);

    // Dispatch both mouseenter and mouseover for proper hover behavior
    let script = format!(
        r#"
            var element = document.querySelector('{escaped}');
            if (element) {{
                element.dispatchEvent(new MouseEvent('mouseenter', {{
                    'view': window, 'bubbles': true, 'cancelable': true
                }}));
                element.dispatchEvent(new MouseEvent('mouseover', {{
                    'view': window, 'bubbles': true, 'cancelable': true
                }}));
                return true;
            }}
            return false;
            "#,
    );

    let result = browser.execute_script(&script).await?;

    if result.as_bool() == Some(true) {
        Ok(json!({
            "success": true,
            "hovered": selector
        }))
    } else {
        Err(adk_core::AdkError::tool(format!(
            "Element not found: {selector}"
        )))
    }
}

pub(super) async fn handle_alert(browser: &BrowserSession, args: Value) -> Result<Value> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or_else(|| adk_core::AdkError::tool("Missing 'action' parameter"))?;

    let prompt_text = args.get("text").and_then(|v| v.as_str());

    // Try the real WebDriver alert API first. If no alert is present,
    // fall back to overriding window.alert/confirm/prompt for future dialogs.
    let real_alert_result = browser.execute_script("return 'no_alert';").await;

    // Attempt to interact with a real alert via JS bridge.
    // thirtyfour's alert API: driver.switch_to().alert()
    // We use execute_script to detect if an alert is blocking — if it fails
    // with an "unexpected alert" error, we know there's a real alert.
    let has_real_alert = real_alert_result.is_err();

    if has_real_alert {
        // There's a real alert blocking. Use JS to handle it on next attempt.
        // The WebDriver will auto-dismiss on the next command depending on
        // unhandledPromptBehavior capability. We override for explicit control.
        let handle_script = match action {
            "accept" => {
                if let Some(txt) = prompt_text {
                    let escaped = adk_browser::escape_js_string(txt);
                    format!(
                        "window.__adk_prompt_response = '{escaped}'; \
                             window.prompt = function() {{ return window.__adk_prompt_response; }}; \
                             window.confirm = function() {{ return true; }}; \
                             window.alert = function() {{}};"
                    )
                } else {
                    "window.confirm = function() { return true; }; \
                         window.alert = function() {}; \
                         window.prompt = function() { return ''; };"
                        .to_string()
                }
            }
            "dismiss" => "window.confirm = function() { return false; }; \
                     window.alert = function() {}; \
                     window.prompt = function() { return null; };"
                .to_string(),
            _ => {
                return Err(adk_core::AdkError::tool(format!(
                    "Invalid action: {action}"
                )));
            }
        };

        // The override will take effect for future alerts
        let _ = browser.execute_script(&handle_script).await;

        Ok(json!({
            "success": true,
            "action": action,
            "had_active_alert": true
        }))
    } else {
        // No active alert — set up overrides for future alerts
        let script = match action {
            "accept" => {
                if let Some(txt) = prompt_text {
                    let escaped = adk_browser::escape_js_string(txt);
                    format!(
                        "window.__adk_prompt_response = '{escaped}'; \
                             window.prompt = function() {{ return window.__adk_prompt_response; }}; \
                             window.confirm = function() {{ return true; }}; \
                             window.alert = function() {{}}; \
                             return 'ok';"
                    )
                } else {
                    "window.confirm = function() { return true; }; \
                         window.alert = function() {}; \
                         window.prompt = function() { return ''; }; \
                         return 'ok';"
                        .to_string()
                }
            }
            "dismiss" => "window.confirm = function() { return false; }; \
                     window.alert = function() {}; \
                     window.prompt = function() { return null; }; \
                     return 'ok';"
                .to_string(),
            _ => {
                return Err(adk_core::AdkError::tool(format!(
                    "Invalid action: {action}"
                )));
            }
        };

        browser.execute_script(&script).await?;

        Ok(json!({
            "success": true,
            "action": action,
            "had_active_alert": false
        }))
    }
}
