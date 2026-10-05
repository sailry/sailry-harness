//! Primitive adapters adapted from adk-browser b3e360bbf49ca39d49cccba182c0f7cb4d251970.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
use adk_browser::BrowserSession;
use adk_core::Result;
use serde_json::{Value, json};

pub(super) async fn text(browser: &BrowserSession, args: Value) -> Result<Value> {
    let selector = args
        .get("selector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| adk_core::AdkError::tool("Missing 'selector' parameter"))?;

    let all = args.get("all").and_then(|v| v.as_bool()).unwrap_or(false);

    if all {
        let elements = browser.find_elements(selector).await?;
        let mut texts = Vec::new();

        for element in elements {
            if let Ok(text) = element.text().await {
                texts.push(text);
            }
        }

        Ok(json!({
            "success": true,
            "texts": texts,
            "count": texts.len()
        }))
    } else {
        let text = browser.get_text(selector).await?;

        Ok(json!({
            "success": true,
            "text": text
        }))
    }
}

pub(super) async fn attribute(browser: &BrowserSession, args: Value) -> Result<Value> {
    let selector = args
        .get("selector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| adk_core::AdkError::tool("Missing 'selector' parameter"))?;

    let attribute = args
        .get("attribute")
        .and_then(|v| v.as_str())
        .ok_or_else(|| adk_core::AdkError::tool("Missing 'attribute' parameter"))?;

    let value = browser.get_attribute(selector, attribute).await?;

    Ok(json!({
        "success": true,
        "attribute": attribute,
        "value": value
    }))
}

pub(super) async fn links(browser: &BrowserSession, args: Value) -> Result<Value> {
    let container = args.get("selector").and_then(|v| v.as_str());
    let include_text = args
        .get("include_text")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let link_selector = if let Some(sel) = container {
        format!("{} a[href]", sel)
    } else {
        "a[href]".to_string()
    };

    let elements = browser.find_elements(&link_selector).await?;
    let mut links = Vec::new();

    for element in elements {
        let href = element.attr("href").await.ok().flatten();
        let text = if include_text {
            element.text().await.ok()
        } else {
            None
        };

        if let Some(href) = href {
            links.push(json!({
                "href": href,
                "text": text
            }));
        }
    }

    Ok(json!({
        "success": true,
        "links": links,
        "count": links.len()
    }))
}

pub(super) async fn page_info(browser: &BrowserSession, _args: Value) -> Result<Value> {
    let url = browser.current_url().await?;
    let title = browser.title().await?;

    Ok(json!({
        "success": true,
        "url": url,
        "title": title
    }))
}

pub(super) async fn page_source(browser: &BrowserSession, args: Value) -> Result<Value> {
    let max_length = args
        .get("max_length")
        .and_then(|v| v.as_u64())
        .unwrap_or(50000) as usize;

    let source = browser.page_source().await?;
    let total_length = source.len();
    let truncated = total_length > max_length;
    let html = if truncated {
        source.chars().take(max_length).collect::<String>()
    } else {
        source
    };

    Ok(json!({
        "success": true,
        "html": html,
        "truncated": truncated,
        "total_length": total_length,
        "returned_length": html.len()
    }))
}
