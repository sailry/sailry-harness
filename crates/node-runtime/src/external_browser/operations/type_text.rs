//! Primitive adapters adapted from adk-browser b3e360bbf49ca39d49cccba182c0f7cb4d251970.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
use adk_browser::BrowserSession;
use adk_core::Result;
use serde_json::{Value, json};

pub(super) async fn r#type(browser: &BrowserSession, args: Value) -> Result<Value> {
    let selector = args
        .get("selector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| adk_core::AdkError::tool("Missing 'selector' parameter"))?;

    let text = args
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| adk_core::AdkError::tool("Missing 'text' parameter"))?;

    let clear_first = args
        .get("clear_first")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let press_enter = args
        .get("press_enter")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Wait for element
    let element = browser.wait_for_element(selector, 10).await?;

    // Clear if requested
    if clear_first {
        element
            .clear()
            .await
            .map_err(|e| adk_core::AdkError::tool(format!("Clear failed: {}", e)))?;
    }

    // Type the text
    element
        .send_keys(text)
        .await
        .map_err(|e| adk_core::AdkError::tool(format!("Type failed: {}", e)))?;

    // Press Enter if requested
    if press_enter {
        element
            .send_keys("\n")
            .await
            .map_err(|e| adk_core::AdkError::tool(format!("Enter key failed: {}", e)))?;
    }

    // Get the current value
    let field_value = element
        .attr("value")
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| text.to_string());

    // Include page context so the agent knows the current state
    let context = browser.page_context().await.unwrap_or_default();

    Ok(json!({
        "success": true,
        "typed_text": text,
        "field_value": field_value,
        "page": context
    }))
}

pub(super) async fn clear(browser: &BrowserSession, args: Value) -> Result<Value> {
    let selector = args
        .get("selector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| adk_core::AdkError::tool("Missing 'selector' parameter"))?;

    browser.clear(selector).await?;

    let context = browser.page_context().await.unwrap_or_default();

    Ok(json!({
        "success": true,
        "cleared": selector,
        "page": context
    }))
}

pub(super) async fn select(browser: &BrowserSession, args: Value) -> Result<Value> {
    let selector = args
        .get("selector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| adk_core::AdkError::tool("Missing 'selector' parameter"))?;

    let value = args.get("value").and_then(|v| v.as_str());
    let text = args.get("text").and_then(|v| v.as_str());
    let index = args.get("index").and_then(|v| v.as_u64());

    let escaped_selector = adk_browser::escape_js_string(selector);

    // Build the appropriate selector for the option
    let option_selector = if let Some(val) = value {
        let escaped_val = adk_browser::escape_js_string(val);
        format!("{selector} option[value='{escaped_val}']")
    } else if let Some(txt) = text {
        let escaped_txt = adk_browser::escape_js_string(txt);
        // Use XPath for text matching isn't available, use JS instead
        let script = format!(
            r#"
                var select = document.querySelector('{escaped_selector}');
                for (var i = 0; i < select.options.length; i++) {{
                    if (select.options[i].text === '{escaped_txt}') {{
                        select.selectedIndex = i;
                        select.dispatchEvent(new Event('change', {{ bubbles: true }}));
                        return true;
                    }}
                }}
                return false;
                "#,
        );

        let result = browser.execute_script(&script).await?;
        if result.as_bool() == Some(true) {
            let context = browser.page_context().await.unwrap_or_default();
            return Ok(json!({
                "success": true,
                "selected_text": txt,
                "page": context
            }));
        } else {
            return Err(adk_core::AdkError::tool(format!(
                "Option with text '{}' not found",
                txt
            )));
        }
    } else if let Some(idx) = index {
        let script = format!(
            r#"
                var select = document.querySelector('{escaped_selector}');
                if (select && select.options.length > {idx}) {{
                    select.selectedIndex = {idx};
                    select.dispatchEvent(new Event('change', {{ bubbles: true }}));
                    return select.options[{idx}].text;
                }}
                return null;
                "#,
        );

        let result = browser.execute_script(&script).await?;
        if let Some(selected_text) = result.as_str() {
            let context = browser.page_context().await.unwrap_or_default();
            return Ok(json!({
                "success": true,
                "selected_text": selected_text,
                "selected_index": idx,
                "page": context
            }));
        } else {
            return Err(adk_core::AdkError::tool(format!(
                "Option at index {} not found",
                idx
            )));
        }
    } else {
        return Err(adk_core::AdkError::tool(
            "Must specify 'value', 'text', or 'index'",
        ));
    };

    // Click the option
    browser.click(&option_selector).await?;

    let context = browser.page_context().await.unwrap_or_default();

    Ok(json!({
        "success": true,
        "selected_value": value,
        "page": context
    }))
}
