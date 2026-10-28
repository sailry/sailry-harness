//! Adapted from ADK-Rust 675c02b6c0df6d632b953a06a5a12d1f814b7780.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
//! The managed Chromium WebDriver connection.

use super::unavailable;
use sailry_protocol::Fault;
mod cookies;
mod input;
mod javascript;
mod pages;
#[cfg(test)]
mod tests;
mod windows;
pub(super) use javascript::escape as escape_js_string;
use std::time::Duration;
use thirtyfour::prelude::*;
use tokio::sync::RwLock;

pub(super) struct Config {
    pub webdriver_url: String,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub page_load_timeout_secs: u64,
    pub script_timeout_secs: u64,
    pub implicit_wait_secs: u64,
    pub browser_args: Vec<String>,
    pub chrome_options: std::collections::BTreeMap<String, serde_json::Value>,
}

/// State information about an element.
#[derive(Debug, Clone)]
pub(super) struct ElementState {
    /// Whether the element is displayed (visible).
    pub is_displayed: bool,
    /// Whether the element is enabled (not disabled).
    pub is_enabled: bool,
    /// Whether the element is selected (for checkboxes, radio buttons, options).
    pub is_selected: bool,
    /// Whether the element is clickable (displayed and enabled).
    pub is_clickable: bool,
}

/// A browser session that wraps thirtyfour's WebDriver.
///
/// The managed browser owner controls startup and shares this connection across operations.
pub(super) struct Session {
    driver: RwLock<Option<WebDriver>>,
    config: Config,
}

impl Session {
    /// Create a new browser session with the given configuration.
    ///
    /// Note: This does not start the browser immediately.
    /// Call `start()` to initialize the WebDriver connection.
    pub(super) fn new(config: Config) -> Self {
        Self {
            driver: RwLock::new(None),
            config,
        }
    }

    /// Start the browser session, replacing a WebDriver connection that no longer responds.
    ///
    /// An existing session that answers a `title` request is kept. One that does not is
    /// dropped, which quits it, and a new session is created from the configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the capabilities cannot be built, the WebDriver server refuses
    /// the session, or a timeout or viewport setting is rejected.
    pub(super) async fn start(&self) -> Result<(), Fault> {
        let mut driver_guard = self.driver.write().await;

        if let Some(driver) = driver_guard.as_ref()
            && driver.title().await.is_ok()
        {
            return Ok(());
        }
        *driver_guard = None;

        let caps = self.capabilities()?;
        let driver = WebDriver::new(&self.config.webdriver_url, caps)
            .await
            .map_err(|e| unavailable(format!("Failed to start browser: {}", e)))?;

        // Set timeouts
        driver
            .set_page_load_timeout(Duration::from_secs(self.config.page_load_timeout_secs))
            .await
            .map_err(|e| unavailable(format!("Failed to set page load timeout: {}", e)))?;

        driver
            .set_script_timeout(Duration::from_secs(self.config.script_timeout_secs))
            .await
            .map_err(|e| unavailable(format!("Failed to set script timeout: {}", e)))?;

        driver
            .set_implicit_wait_timeout(Duration::from_secs(self.config.implicit_wait_secs))
            .await
            .map_err(|e| unavailable(format!("Failed to set implicit wait: {}", e)))?;

        // Set viewport size
        driver
            .set_window_rect(
                0,
                0,
                self.config.viewport_width,
                self.config.viewport_height,
            )
            .await
            .map_err(|e| unavailable(format!("Failed to set viewport: {}", e)))?;

        *driver_guard = Some(driver);
        Ok(())
    }

    /// Stop the browser session.
    pub(super) async fn stop(&self) -> Result<(), Fault> {
        let mut driver_guard = self.driver.write().await;

        if let Some(driver) = driver_guard.take() {
            driver
                .quit()
                .await
                .map_err(|e| unavailable(format!("Failed to quit browser: {}", e)))?;
        }

        Ok(())
    }

    /// Check the connection without creating a browser behind the owner's back.
    async fn ensure_started(&self) -> Result<(), Fault> {
        let guard = self.driver.read().await;
        if let Some(driver) = guard.as_ref()
            && driver.title().await.is_ok()
        {
            return Ok(());
        }
        Err(unavailable(
            "browser session is not running; start or replace the managed session",
        ))
    }

    /// Get a live WebDriver handle after checking the owner-started connection.
    /// All public methods that access the WebDriver MUST go through this.
    ///
    /// Returns a cloned WebDriver handle (cheap — WebDriver is Arc-based internally).
    /// The RwLock is NOT held across the caller's WebDriver operations.
    async fn live_driver(&self) -> Result<WebDriver, Fault> {
        self.ensure_started().await?;

        let guard = self.driver.read().await;
        guard.clone().ok_or_else(|| {
            unavailable("Failed to start browser session: driver is None after ensure_started()")
        })
    }

    fn capabilities(&self) -> Result<Capabilities, Fault> {
        let mut caps = DesiredCapabilities::chrome();
        for arg in ["--headless=new", "--disable-dev-shm-usage"]
            .into_iter()
            .chain(self.config.browser_args.iter().map(String::as_str))
        {
            caps.add_arg(arg).map_err(|error| {
                unavailable(format!("Failed to add browser argument '{arg}': {error}"))
            })?;
        }
        for (name, value) in &self.config.chrome_options {
            // Replacing `args` would silently remove the owner's browser flags.
            if name == "args" {
                return Err(unavailable(
                    "chrome_options cannot set `args`; use browser_args instead",
                ));
            }
            caps.add_experimental_option(name, value.clone())
                .map_err(|error| unavailable(format!("Invalid Chrome option: {error}")))?;
        }
        Ok(caps.into())
    }
}
