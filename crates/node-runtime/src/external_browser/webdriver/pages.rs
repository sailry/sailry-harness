//! Adapted from ADK-Rust 675c02b6c0df6d632b953a06a5a12d1f814b7780.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
use super::*;

impl Session {
    /// Get a snapshot of the current page context (url, title, truncated body text).
    ///
    /// Useful for returning page state after interaction tools.
    pub(in crate::external_browser) async fn page_context(
        &self,
    ) -> Result<serde_json::Value, Fault> {
        let url = self.current_url().await.unwrap_or_default();
        let title = self.title().await.unwrap_or_default();

        // Get truncated visible text (first 2000 chars of body)
        let page_text = self
            .execute_script(
                "return (document.body && document.body.innerText || '').substring(0, 2000);",
            )
            .await
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();

        Ok(serde_json::json!({
            "url": url,
            "title": title,
            "page_text": page_text
        }))
    }

    /// Navigate to a URL.
    pub(in crate::external_browser) async fn navigate(&self, url: &str) -> Result<(), Fault> {
        let driver = self.live_driver().await?;

        driver
            .goto(url)
            .await
            .map_err(|e| unavailable(format!("Navigation failed: {}", e)))?;

        Ok(())
    }

    /// Get the current URL.
    pub(in crate::external_browser) async fn current_url(&self) -> Result<String, Fault> {
        let driver = self.live_driver().await?;

        driver
            .current_url()
            .await
            .map(|u| u.to_string())
            .map_err(|e| unavailable(format!("Failed to get URL: {}", e)))
    }

    /// Get the page title.
    pub(in crate::external_browser) async fn title(&self) -> Result<String, Fault> {
        let driver = self.live_driver().await?;

        driver
            .title()
            .await
            .map_err(|e| unavailable(format!("Failed to get title: {}", e)))
    }

    /// Find an element by CSS selector.
    pub(in crate::external_browser) async fn find_element(
        &self,
        selector: &str,
    ) -> Result<WebElement, Fault> {
        let driver = self.live_driver().await?;

        driver
            .find(By::Css(selector))
            .await
            .map_err(|e| unavailable(format!("Element not found '{}': {}", selector, e)))
    }

    /// Find multiple elements by CSS selector.
    pub(in crate::external_browser) async fn find_elements(
        &self,
        selector: &str,
    ) -> Result<Vec<WebElement>, Fault> {
        let driver = self.live_driver().await?;

        driver
            .find_all(By::Css(selector))
            .await
            .map_err(|e| unavailable(format!("Elements query failed '{}': {}", selector, e)))
    }

    /// Click an element by selector.
    pub(in crate::external_browser) async fn click(&self, selector: &str) -> Result<(), Fault> {
        let element = self.find_element(selector).await?;
        element
            .click()
            .await
            .map_err(|e| unavailable(format!("Click failed on '{}': {}", selector, e)))
    }

    /// Clear an input field.
    pub(in crate::external_browser) async fn clear(&self, selector: &str) -> Result<(), Fault> {
        let element = self.find_element(selector).await?;
        element
            .clear()
            .await
            .map_err(|e| unavailable(format!("Clear failed on '{}': {}", selector, e)))
    }

    /// Get text content of an element.
    pub(in crate::external_browser) async fn get_text(
        &self,
        selector: &str,
    ) -> Result<String, Fault> {
        let element = self.find_element(selector).await?;
        element
            .text()
            .await
            .map_err(|e| unavailable(format!("Get text failed on '{}': {}", selector, e)))
    }

    /// Get an attribute value.
    pub(in crate::external_browser) async fn get_attribute(
        &self,
        selector: &str,
        attribute: &str,
    ) -> Result<Option<String>, Fault> {
        let element = self.find_element(selector).await?;
        element
            .attr(attribute)
            .await
            .map_err(|e| unavailable(format!("Get attribute failed: {}", e)))
    }

    /// Take a screenshot (returns base64-encoded PNG).
    pub(in crate::external_browser) async fn screenshot(&self) -> Result<String, Fault> {
        let driver = self.live_driver().await?;

        let screenshot = driver
            .screenshot_as_png_base64()
            .await
            .map_err(|e| unavailable(format!("Screenshot failed: {}", e)))?;

        Ok(screenshot)
    }

    /// Take a screenshot of a specific element.
    pub(in crate::external_browser) async fn screenshot_element(
        &self,
        selector: &str,
    ) -> Result<String, Fault> {
        let element = self.find_element(selector).await?;
        let screenshot = element
            .screenshot_as_png_base64()
            .await
            .map_err(|e| unavailable(format!("Element screenshot failed: {}", e)))?;

        Ok(screenshot)
    }

    /// Execute JavaScript and return result.
    pub(in crate::external_browser) async fn execute_script(
        &self,
        script: &str,
    ) -> Result<serde_json::Value, Fault> {
        let driver = self.live_driver().await?;

        let result = driver
            .execute(script, vec![])
            .await
            .map_err(|e| unavailable(format!("Script execution failed: {}", e)))?;

        // thirtyfour's ScriptRet provides .json() which returns &Value directly
        Ok(result.json().clone())
    }

    /// Execute async JavaScript and return result.
    pub(in crate::external_browser) async fn execute_async_script(
        &self,
        script: &str,
    ) -> Result<serde_json::Value, Fault> {
        let driver = self.live_driver().await?;

        let result = driver
            .execute_async(script, vec![])
            .await
            .map_err(|e| unavailable(format!("Async script failed: {}", e)))?;

        Ok(result.json().clone())
    }

    /// Wait for an element to be present.
    pub(in crate::external_browser) async fn wait_for_element(
        &self,
        selector: &str,
        timeout_secs: u64,
    ) -> Result<WebElement, Fault> {
        let driver = self.live_driver().await?;

        driver
            .query(By::Css(selector))
            .wait(
                Duration::from_secs(timeout_secs),
                Duration::from_millis(100),
            )
            .first()
            .await
            .map_err(|e| {
                unavailable(format!(
                    "Timeout waiting for '{}' after {}s: {}",
                    selector, timeout_secs, e
                ))
            })
    }

    /// Wait for an element to be clickable.
    pub(in crate::external_browser) async fn wait_for_clickable(
        &self,
        selector: &str,
        timeout_secs: u64,
    ) -> Result<WebElement, Fault> {
        let driver = self.live_driver().await?;

        driver
            .query(By::Css(selector))
            .wait(
                Duration::from_secs(timeout_secs),
                Duration::from_millis(100),
            )
            .and_clickable()
            .first()
            .await
            .map_err(|e| {
                unavailable(format!(
                    "Timeout waiting for clickable '{}': {}",
                    selector, e
                ))
            })
    }

    /// Get page source HTML.
    pub(in crate::external_browser) async fn page_source(&self) -> Result<String, Fault> {
        let driver = self.live_driver().await?;

        driver
            .source()
            .await
            .map_err(|e| unavailable(format!("Failed to get page source: {}", e)))
    }

    /// Go back in history.
    pub(in crate::external_browser) async fn back(&self) -> Result<(), Fault> {
        let driver = self.live_driver().await?;

        driver
            .back()
            .await
            .map_err(|e| unavailable(format!("Back navigation failed: {}", e)))
    }

    /// Go forward in history.
    pub(in crate::external_browser) async fn forward(&self) -> Result<(), Fault> {
        let driver = self.live_driver().await?;

        driver
            .forward()
            .await
            .map_err(|e| unavailable(format!("Forward navigation failed: {}", e)))
    }

    /// Refresh the current page.
    pub(in crate::external_browser) async fn refresh(&self) -> Result<(), Fault> {
        let driver = self.live_driver().await?;

        driver
            .refresh()
            .await
            .map_err(|e| unavailable(format!("Refresh failed: {}", e)))
    }
}
