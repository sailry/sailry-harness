//! Adapted from ADK-Rust 675c02b6c0df6d632b953a06a5a12d1f814b7780.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
use super::*;

impl Session {
    /// Get all cookies.
    pub(in crate::external_browser) async fn get_all_cookies(
        &self,
    ) -> Result<Vec<serde_json::Value>, Fault> {
        let driver = self.live_driver().await?;

        let cookies = driver
            .get_all_cookies()
            .await
            .map_err(|e| unavailable(format!("Failed to get cookies: {}", e)))?;

        Ok(cookies
            .into_iter()
            .map(|c| {
                serde_json::json!({
                    "name": c.name,
                    "value": c.value,
                    "domain": c.domain,
                    "path": c.path,
                    "secure": c.secure,
                })
            })
            .collect())
    }

    /// Get a cookie by name.
    pub(in crate::external_browser) async fn get_cookie(
        &self,
        name: &str,
    ) -> Result<serde_json::Value, Fault> {
        let driver = self.live_driver().await?;

        let cookie = driver
            .get_named_cookie(name)
            .await
            .map_err(|e| unavailable(format!("Failed to get cookie '{}': {}", name, e)))?;

        Ok(serde_json::json!({
            "name": cookie.name,
            "value": cookie.value,
            "domain": cookie.domain,
            "path": cookie.path,
            "secure": cookie.secure,
        }))
    }

    /// Add a cookie.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::external_browser) async fn add_cookie(
        &self,
        name: &str,
        value: &str,
        domain: Option<&str>,
        path: Option<&str>,
        secure: Option<bool>,
        expiry: Option<i64>,
    ) -> Result<(), Fault> {
        let driver = self.live_driver().await?;

        let mut cookie = thirtyfour::Cookie::new(name, value);
        if let Some(d) = domain {
            cookie.set_domain(d);
        }
        if let Some(p) = path {
            cookie.set_path(p);
        }
        if let Some(s) = secure {
            cookie.set_secure(s);
        }
        if let Some(exp) = expiry {
            // thirtyfour Cookie expiry expects seconds since epoch
            cookie.set_expiry(exp);
        }

        driver
            .add_cookie(cookie)
            .await
            .map_err(|e| unavailable(format!("Failed to add cookie: {e}")))
    }

    /// Delete a cookie.
    pub(in crate::external_browser) async fn delete_cookie(&self, name: &str) -> Result<(), Fault> {
        let driver = self.live_driver().await?;

        driver
            .delete_cookie(name)
            .await
            .map_err(|e| unavailable(format!("Failed to delete cookie: {}", e)))
    }

    /// Delete all cookies.
    pub(in crate::external_browser) async fn delete_all_cookies(&self) -> Result<(), Fault> {
        let driver = self.live_driver().await?;

        driver
            .delete_all_cookies()
            .await
            .map_err(|e| unavailable(format!("Failed to delete all cookies: {}", e)))
    }
}
