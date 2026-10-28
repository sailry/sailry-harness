//! Adapted from ADK-Rust 675c02b6c0df6d632b953a06a5a12d1f814b7780.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
use super::*;
use thirtyfour::common::print::{PrintOrientation, PrintParameters};

impl Session {
    /// Switch to frame by index.
    pub(in crate::external_browser) async fn switch_to_frame_by_index(
        &self,
        index: u16,
    ) -> Result<(), Fault> {
        let driver = self.live_driver().await?;

        driver
            .enter_frame(index)
            .await
            .map_err(|e| unavailable(format!("Failed to switch to frame {}: {}", index, e)))
    }

    /// Switch to frame by selector.
    pub(in crate::external_browser) async fn switch_to_frame_by_selector(
        &self,
        selector: &str,
    ) -> Result<(), Fault> {
        let element = self.find_element(selector).await?;

        element
            .enter_frame()
            .await
            .map_err(|e| unavailable(format!("Failed to switch to frame: {}", e)))
    }

    /// Switch to parent frame.
    pub(in crate::external_browser) async fn switch_to_parent_frame(&self) -> Result<(), Fault> {
        let driver = self.live_driver().await?;

        driver
            .enter_parent_frame()
            .await
            .map_err(|e| unavailable(format!("Failed to switch to parent frame: {}", e)))
    }

    /// Switch to default content.
    pub(in crate::external_browser) async fn switch_to_default_content(&self) -> Result<(), Fault> {
        let driver = self.live_driver().await?;

        driver
            .enter_default_frame()
            .await
            .map_err(|e| unavailable(format!("Failed to switch to default content: {}", e)))
    }

    /// Drag and drop.
    pub(in crate::external_browser) async fn drag_and_drop(
        &self,
        source_selector: &str,
        target_selector: &str,
    ) -> Result<(), Fault> {
        let source = self.find_element(source_selector).await?;
        let target = self.find_element(target_selector).await?;

        // Use JavaScript-based drag and drop for broader compatibility
        source
            .js_drag_to(&target)
            .await
            .map_err(|e| unavailable(format!("Drag and drop failed: {}", e)))
    }

    /// Right-click (context click).
    pub(in crate::external_browser) async fn right_click(
        &self,
        selector: &str,
    ) -> Result<(), Fault> {
        let escaped = escape_js_string(selector);
        let script = format!(
            r#"
            var element = document.querySelector('{escaped}');
            if (element) {{
                var event = new MouseEvent('contextmenu', {{
                    'view': window,
                    'bubbles': true,
                    'cancelable': true
                }});
                element.dispatchEvent(event);
                return true;
            }}
            return false;
            "#,
        );

        let result = self.execute_script(&script).await?;
        if result.as_bool() != Some(true) {
            return Err(unavailable(format!("Element not found: {}", selector)));
        }
        Ok(())
    }

    /// Focus an element.
    pub(in crate::external_browser) async fn focus_element(
        &self,
        selector: &str,
    ) -> Result<(), Fault> {
        let element = self.find_element(selector).await?;
        element
            .focus()
            .await
            .map_err(|e| unavailable(format!("Focus failed: {}", e)))
    }

    /// Get element state.
    pub(in crate::external_browser) async fn get_element_state(
        &self,
        selector: &str,
    ) -> Result<ElementState, Fault> {
        let element = self.find_element(selector).await?;

        let is_displayed = element.is_displayed().await.unwrap_or(false);
        let is_enabled = element.is_enabled().await.unwrap_or(false);
        let is_selected = element.is_selected().await.unwrap_or(false);
        let is_clickable = element.is_clickable().await.unwrap_or(false);

        Ok(ElementState {
            is_displayed,
            is_enabled,
            is_selected,
            is_clickable,
        })
    }

    /// Press a key, optionally with modifier keys (Ctrl, Alt, Shift, Meta).
    pub(in crate::external_browser) async fn press_key(
        &self,
        key: &str,
        selector: Option<&str>,
        modifiers: &[&str],
    ) -> Result<(), Fault> {
        // Map modifier names to WebDriver key codes
        let modifier_codes: Vec<&str> = modifiers
            .iter()
            .filter_map(|m| match m.to_lowercase().as_str() {
                "ctrl" | "control" => Some("\u{E009}"),
                "alt" => Some("\u{E00A}"),
                "shift" => Some("\u{E008}"),
                "meta" | "command" | "cmd" => Some("\u{E03D}"),
                _ => None,
            })
            .collect();

        let key_str = match key.to_lowercase().as_str() {
            "enter" => "\u{E007}",
            "tab" => "\u{E004}",
            "escape" | "esc" => "\u{E00C}",
            "backspace" => "\u{E003}",
            "delete" => "\u{E017}",
            "arrowup" | "up" => "\u{E013}",
            "arrowdown" | "down" => "\u{E015}",
            "arrowleft" | "left" => "\u{E012}",
            "arrowright" | "right" => "\u{E014}",
            "home" => "\u{E011}",
            "end" => "\u{E010}",
            "pageup" => "\u{E00E}",
            "pagedown" => "\u{E00F}",
            "space" => " ",
            _ => key,
        };

        // Build the full key sequence: modifiers down + key + modifiers up
        let mut key_sequence = String::new();
        for code in &modifier_codes {
            key_sequence.push_str(code);
        }
        key_sequence.push_str(key_str);
        // Release modifiers in reverse order
        for code in modifier_codes.iter().rev() {
            key_sequence.push_str(code);
        }

        if let Some(sel) = selector {
            let element = self.find_element(sel).await?;
            element
                .send_keys(&key_sequence)
                .await
                .map_err(|e| unavailable(format!("Key press failed: {e}")))?;
        } else {
            // Send to active element via WebDriver
            let driver = self.live_driver().await?;

            let active = driver
                .active_element()
                .await
                .map_err(|e| unavailable(format!("No active element: {e}")))?;
            active
                .send_keys(&key_sequence)
                .await
                .map_err(|e| unavailable(format!("Key press failed: {e}")))?;
        }

        Ok(())
    }

    /// Upload a file.
    pub(in crate::external_browser) async fn upload_file(
        &self,
        selector: &str,
        file_path: &str,
    ) -> Result<(), Fault> {
        let element = self.find_element(selector).await?;
        element
            .send_keys(file_path)
            .await
            .map_err(|e| unavailable(format!("File upload failed: {}", e)))
    }

    /// Print page to PDF.
    pub(in crate::external_browser) async fn print_to_pdf(
        &self,
        landscape: bool,
        scale: f64,
    ) -> Result<String, Fault> {
        let driver = self.live_driver().await?;

        let params = PrintParameters {
            orientation: if landscape {
                PrintOrientation::Landscape
            } else {
                PrintOrientation::Portrait
            },
            scale,
            ..Default::default()
        };

        driver
            .print_page_base64(params)
            .await
            .map_err(|e| unavailable(format!("Print to PDF failed: {}", e)))
    }
}
