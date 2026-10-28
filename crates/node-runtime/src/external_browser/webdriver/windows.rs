//! Adapted from ADK-Rust 675c02b6c0df6d632b953a06a5a12d1f814b7780.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
use super::*;

impl Session {
    /// List all windows/tabs.
    pub(in crate::external_browser) async fn list_windows(
        &self,
    ) -> Result<(Vec<String>, String), Fault> {
        let driver = self.live_driver().await?;

        let windows = driver
            .windows()
            .await
            .map_err(|e| unavailable(format!("Failed to get windows: {}", e)))?;

        let current = driver
            .window()
            .await
            .map_err(|e| unavailable(format!("Failed to get current window: {}", e)))?;

        Ok((
            windows.into_iter().map(|w| w.to_string()).collect(),
            current.to_string(),
        ))
    }

    /// Open a new tab.
    pub(in crate::external_browser) async fn new_tab(&self) -> Result<String, Fault> {
        let driver = self.live_driver().await?;

        let handle = driver
            .new_tab()
            .await
            .map_err(|e| unavailable(format!("Failed to open new tab: {}", e)))?;

        Ok(handle.to_string())
    }

    /// Open a new window.
    pub(in crate::external_browser) async fn new_window(&self) -> Result<String, Fault> {
        let driver = self.live_driver().await?;

        let handle = driver
            .new_window()
            .await
            .map_err(|e| unavailable(format!("Failed to open new window: {}", e)))?;

        Ok(handle.to_string())
    }

    /// Switch to a window by handle.
    pub(in crate::external_browser) async fn switch_to_window(
        &self,
        handle: &str,
    ) -> Result<(), Fault> {
        let driver = self.live_driver().await?;

        let window_handle = thirtyfour::WindowHandle::from(handle.to_string());
        driver
            .switch_to_window(window_handle)
            .await
            .map_err(|e| unavailable(format!("Failed to switch window: {}", e)))
    }

    /// Close the current window.
    pub(in crate::external_browser) async fn close_window(&self) -> Result<(), Fault> {
        let driver = self.live_driver().await?;

        driver
            .close_window()
            .await
            .map_err(|e| unavailable(format!("Failed to close window: {}", e)))
    }

    /// Maximize window.
    pub(in crate::external_browser) async fn maximize_window(&self) -> Result<(), Fault> {
        let driver = self.live_driver().await?;

        driver
            .maximize_window()
            .await
            .map_err(|e| unavailable(format!("Failed to maximize window: {}", e)))
    }

    /// Minimize window.
    pub(in crate::external_browser) async fn minimize_window(&self) -> Result<(), Fault> {
        let driver = self.live_driver().await?;

        driver
            .minimize_window()
            .await
            .map_err(|e| unavailable(format!("Failed to minimize window: {}", e)))
    }

    /// Set window size and position.
    pub(in crate::external_browser) async fn set_window_rect(
        &self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), Fault> {
        let driver = self.live_driver().await?;

        driver
            .set_window_rect(x as i64, y as i64, width, height)
            .await
            .map_err(|e| unavailable(format!("Failed to set window rect: {}", e)))
    }
}
