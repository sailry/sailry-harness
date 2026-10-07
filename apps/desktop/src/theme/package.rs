//! Current v1 desktop theme packages. Resources are local, licensed raster images.
mod resources;
#[cfg(test)]
mod tests;

use gpui_kit::{RenderImage, component::theme::ThemeConfig};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub const SLOTS: &[&str] = &[
    "background.window",
    "background.sidebar",
    "background.content",
    "background.card.explore",
    "background.card.build",
    "background.card.review",
    "background.card.plan",
    "brand",
    "banner.app",
    "banner.settings",
    "banner.conversation.top",
    "banner.conversation.bottom",
    "banner.new_session",
    "hero.new_session",
    "composer.top_left",
    "composer.bottom_right",
    "button.send",
    "button.stop",
    "button.new_session",
    "message.user.top_left",
    "message.user.bottom_right",
    "message.assistant.top_left",
    "message.assistant.bottom_right",
    "icon.send",
    "icon.stop",
    "icon.explore",
    "icon.build",
    "icon.review",
    "icon.plan",
    "icon.group.hosts",
    "icon.group.activity",
    "icon.group.unassigned",
    "icon.group.projects",
    "icon.group.recent",
    "icon.conversation",
    "icon.activity",
    "icon.files",
    "icon.git",
    "icon.terminal",
    "icon.ssh",
    "icon.database",
    "icon.usage",
    "icon.settings",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct License {
    pub author: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The theme's release version, independent of the v1 manifest format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_version: Option<String>,
    /// Applies to the package and assets without an individual license.
    pub license: License,
    pub light: Variant,
    pub dark: Variant,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    #[serde(deserialize_with = "read_theme")]
    pub theme: ThemeConfig,
    #[serde(default)]
    pub assets: BTreeMap<String, Decoration>,
    #[serde(default)]
    pub composer: Composer,
    #[serde(default)]
    pub cards: BTreeMap<String, Card>,
    #[serde(default)]
    pub messages: BTreeMap<String, Message>,
    #[serde(default)]
    pub wordmark: Wordmark,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub border: Option<String>,
    pub border_width: Option<u8>,
    pub radius: Option<u8>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wordmark {
    pub foreground: Option<String>,
    pub glow: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Composer {
    /// Uses Kit's input border color; zero preserves the borderless surface.
    #[serde(default)]
    pub border_width: u8,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub detail: Option<String>,
    pub icon: Option<String>,
    pub hover: Option<String>,
    pub active: Option<String>,
    pub border: Option<String>,
    #[serde(default)]
    pub border_width: u8,
    /// Omitted radii follow Kit's large surface radius.
    pub radius: Option<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decoration {
    pub path: String,
    #[serde(default)]
    pub fit: Fit,
    /// Horizontal and vertical alignment, from 0 (start) to 1 (end).
    #[serde(default = "center")]
    pub alignment: [f32; 2],
    #[serde(default = "opaque")]
    pub opacity: f32,
    /// Banner, Hero and button-edge height, or square corner size.
    #[serde(default = "height")]
    pub height: u16,
    /// Logical tile width; aspect ratio is preserved independently of window size.
    #[serde(default = "tile_size")]
    pub tile_size: u16,
    pub license: Option<License>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    #[default]
    Cover,
    Contain,
    Fill,
    Tile,
}

fn center() -> [f32; 2] {
    [0.5, 0.5]
}
fn opaque() -> f32 {
    1.
}
fn height() -> u16 {
    64
}
fn tile_size() -> u16 {
    256
}

pub struct Loaded {
    pub manifest: Manifest,
    pub images: BTreeMap<String, Arc<RenderImage>>,
    pub warnings: Vec<String>,
    // A verified snapshot, so importing does not read a changed source tree again.
    files: BTreeMap<String, Vec<u8>>,
}

impl Manifest {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("unsupported theme package version".into());
        }
        if !valid_id(&self.id) || self.id == "builtin" {
            return Err("invalid theme package ID".into());
        }
        if self.name.trim().is_empty()
            || self.name.len() > 160
            || self.name.chars().any(char::is_control)
        {
            return Err("invalid theme package name".into());
        }
        if self.description.as_ref().is_some_and(|description| {
            description.trim().is_empty()
                || description.len() > 4 * 1024
                || description
                    .chars()
                    .any(|value| value.is_control() && !matches!(value, '\n' | '\r' | '\t'))
        }) {
            return Err("invalid theme package description".into());
        }
        if self.package_version.as_ref().is_some_and(|version| {
            version.trim().is_empty() || version.len() > 64 || version.chars().any(char::is_control)
        }) {
            return Err("invalid theme release version".into());
        }
        self.license.validate()?;
        if self.light.theme.mode.is_dark() || !self.dark.theme.mode.is_dark() {
            return Err("theme package requires light and dark variants".into());
        }
        for variant in [&self.light, &self.dark] {
            for (key, message) in &variant.messages {
                if !["user", "assistant"].contains(&key.as_str()) {
                    return Err(format!("unsupported message style: {key}"));
                }
                if message.border_width.is_some_and(|width| width > 4)
                    || message.radius.is_some_and(|radius| radius > 32)
                {
                    return Err(format!("invalid message dimensions: {key}"));
                }
                for color in [&message.background, &message.foreground, &message.border]
                    .into_iter()
                    .flatten()
                {
                    gpui_kit::component::theme::try_parse_color(color)
                        .map_err(|error| error.to_string())?;
                }
            }
            for color in [&variant.wordmark.foreground, &variant.wordmark.glow]
                .into_iter()
                .flatten()
            {
                gpui_kit::component::theme::try_parse_color(color)
                    .map_err(|error| error.to_string())?;
            }
            if variant.composer.border_width > 4 {
                return Err("invalid composer border width".into());
            }
            for (key, card) in &variant.cards {
                if !["explore", "build", "review", "plan"].contains(&key.as_str()) {
                    return Err(format!("unsupported welcome card: {key}"));
                }
                if card.border_width > 4 || card.radius.is_some_and(|radius| radius > 32) {
                    return Err(format!("invalid welcome card dimensions: {key}"));
                }
                for color in [
                    &card.background,
                    &card.foreground,
                    &card.detail,
                    &card.icon,
                    &card.hover,
                    &card.active,
                    &card.border,
                ]
                .into_iter()
                .flatten()
                {
                    gpui_kit::component::theme::try_parse_color(color)
                        .map_err(|error| error.to_string())?;
                }
            }
            for (slot, asset) in &variant.assets {
                if !SLOTS.contains(&slot.as_str()) {
                    return Err(format!("unsupported theme slot: {slot}"));
                }
                if !(32..=1024).contains(&asset.tile_size)
                    || (matches!(asset.fit, Fit::Tile) && !slot.starts_with("background."))
                {
                    return Err(format!("invalid theme tiling: {slot}"));
                }
                resources::validate_path(&asset.path)?;
                let unit = |v: f32| v.is_finite() && (0. ..=1.).contains(&v);
                let max_height = if slot == "hero.new_session" { 240 } else { 96 };
                let min_height = if slot == "button.new_session" { 4 } else { 16 };
                if !unit(asset.opacity)
                    || !asset.alignment.into_iter().all(unit)
                    || !(min_height..=max_height).contains(&asset.height)
                {
                    return Err(format!("invalid theme decoration dimensions: {slot}"));
                }
                if let Some(license) = &asset.license {
                    license.validate()?;
                }
            }
        }
        Ok(())
    }
}

impl License {
    fn validate(&self) -> Result<(), String> {
        if self.name.as_ref().is_some_and(|name| {
            name.trim().is_empty() || name.len() > 160 || name.chars().any(char::is_control)
        }) {
            return Err("invalid theme license name".into());
        }
        if self.author.trim().is_empty()
            || self.author.len() > 512
            || self.author.chars().any(char::is_control)
            || self.text.trim().is_empty()
            || self.text.len() > 16 * 1024
        {
            return Err("theme package requires author and license text".into());
        }
        Ok(())
    }
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && id.as_bytes()[0].is_ascii_alphanumeric()
}

fn read_theme<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<ThemeConfig, D::Error> {
    use gpui_kit::component::theme::try_parse_background;
    use serde::de::Error;
    let value = serde_json::Value::deserialize(deserializer)?;
    let object = value
        .as_object()
        .ok_or_else(|| D::Error::custom("theme must be an object"))?;
    let template = serde_json::to_value(ThemeConfig::default()).map_err(D::Error::custom)?;
    for key in object.keys() {
        if (key == "highlight" && !object[key].is_null()) || template.get(key).is_none() {
            return Err(D::Error::custom(format!("unsupported theme field: {key}")));
        }
    }
    if let Some(colors) = value.get("colors") {
        for (key, color) in colors
            .as_object()
            .ok_or_else(|| D::Error::custom("colors must be an object"))?
        {
            if template["colors"].get(key).is_none() {
                return Err(D::Error::custom(format!("unknown Kit color: {key}")));
            }
            if !color.is_null() {
                try_parse_background(
                    color
                        .as_str()
                        .ok_or_else(|| D::Error::custom("color must be a string"))?,
                )
                .map_err(D::Error::custom)?;
            }
        }
    }
    let theme: ThemeConfig = serde_json::from_value(value).map_err(D::Error::custom)?;
    if [theme.font_size, theme.mono_font_size]
        .into_iter()
        .flatten()
        .any(|size| !size.is_finite() || !(8. ..=32.).contains(&size))
        || [theme.radius, theme.radius_lg]
            .into_iter()
            .flatten()
            .any(|radius| radius > 32)
    {
        return Err(D::Error::custom("invalid theme sizing"));
    }
    Ok(theme)
}

impl Loaded {
    /// Import is strict; installed packages may lose optional images and still supply colors.
    pub fn read(directory: &Path, strict: bool) -> Result<Self, String> {
        resources::load(directory, strict)
    }

    pub fn install(&self, root: &Path) -> Result<(), String> {
        resources::install(self, root).map_err(|error| error.to_string())
    }
}
