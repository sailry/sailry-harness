//! Prepared local SVG resources; the platform owns the hidden rendering surface.
use std::{collections::HashMap, rc::Rc, sync::Arc};

use gpui_kit::{App, Global, Image, ImageFormat, Task, Window};
use serde::{Deserialize, Serialize};
pub(super) mod state;

pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_SVG_BYTES: usize = 2 * 1024 * 1024;

pub fn asset(path: &str) -> Option<&'static [u8]> {
    match path.trim_start_matches('/') {
        "index.html" => Some(include_bytes!("../../assets/visuals/index.html")),
        "config.js" => Some(include_bytes!("../../assets/visuals/config.js")),
        "mermaid.js" => Some(include_bytes!("../../assets/visuals/mermaid.js")),
        "mathjax.js" => Some(include_bytes!("../../assets/visuals/mathjax.js")),
        "fonts.js" => Some(include_bytes!("../../assets/visuals/fonts.js")),
        "render.js" => Some(include_bytes!("../../assets/visuals/render.js")),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Mermaid,
    InlineMath,
    DisplayMath,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub kind: Kind,
    pub source: String,
    pub dark: bool,
    pub foreground: String,
    pub background: String,
    pub accent: String,
    pub font_size: f32,
}

impl Request {
    pub fn validate(&self) -> Result<(), String> {
        if self.source.is_empty() || self.source.len() > MAX_SOURCE_BYTES {
            return Err("Visual source exceeds the supported size".into());
        }
        if !self.font_size.is_finite() || !(8.0..=96.0).contains(&self.font_size) {
            return Err("Visual font size is invalid".into());
        }
        if [&self.foreground, &self.background, &self.accent]
            .iter()
            .any(|color| {
                color.len() != 7
                    || !color.starts_with('#')
                    || !color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        {
            return Err("Visual colors are invalid".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Svg {
    pub source: String,
    pub width: f32,
    pub height: f32,
    pub baseline: Option<f32>,
}

impl Svg {
    pub fn validate(&self) -> Result<(), String> {
        if self.source.len() > MAX_SVG_BYTES
            || [self.width, self.height]
                .iter()
                .any(|value| !value.is_finite() || !(0.0..=4096.0).contains(value) || *value == 0.0)
            || self
                .baseline
                .is_some_and(|value| !value.is_finite() || !(0.0..=self.height).contains(&value))
        {
            return Err("Rendered visual exceeds the supported size".into());
        }
        validate_source(&self.source)
    }
}

/// Checks that SVG text is well formed and references no external resources or scripts.
pub fn validate_source(source: &str) -> Result<(), String> {
    if source.len() > MAX_SVG_BYTES {
        return Err("Rendered visual exceeds the supported size".into());
    }
    let document = roxmltree::Document::parse(source)
        .map_err(|_| "Rendered visual is not valid SVG".to_owned())?;
    if document.root_element().tag_name().name() != "svg"
        || document.root_element().tag_name().namespace() != Some("http://www.w3.org/2000/svg")
    {
        return Err("Rendered visual is not SVG".into());
    }
    for node in document.descendants().filter(|node| node.is_element()) {
        if matches!(
            node.tag_name().name(),
            "script"
                | "foreignObject"
                | "iframe"
                | "image"
                | "a"
                | "animate"
                | "animateTransform"
                | "set"
        ) {
            return Err("Rendered visual contains unsupported elements".into());
        }
        for attribute in node.attributes() {
            let name = attribute.name().to_ascii_lowercase();
            let value = attribute.value().trim();
            if name.starts_with("on")
                || (matches!(name.as_str(), "href" | "src") && !value.starts_with('#'))
                || value.to_ascii_lowercase().contains("@import")
                || external_url(value)
            {
                return Err("Rendered visual contains external resources".into());
            }
        }
        if node.tag_name().name() == "style"
            && node.text().is_some_and(|text| {
                text.to_ascii_lowercase().contains("@import") || external_url(text)
            })
        {
            return Err("Rendered visual contains external styles".into());
        }
    }
    Ok(())
}

fn external_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let mut remainder = lower.as_str();
    while let Some(index) = remainder.find("url(") {
        remainder = &remainder[index + 4..];
        let Some(end) = remainder.find(')') else {
            return true;
        };
        if !remainder[..end]
            .trim()
            .trim_matches(['\'', '"'])
            .starts_with('#')
        {
            return true;
        }
        remainder = &remainder[end + 1..];
    }
    false
}

pub type Render = dyn Fn(Request, &mut Window, &mut App) -> Task<Result<Svg, String>>;

#[derive(Clone)]
pub struct Renderer(pub Rc<Render>);
impl Global for Renderer {}

#[derive(Clone)]
pub struct Prepared {
    pub image: Arc<Image>,
    pub width: f32,
    pub height: f32,
    pub baseline: Option<f32>,
    pub font_size: f32,
}

impl Prepared {
    pub fn new(svg: Svg, font_size: f32) -> Result<Self, String> {
        svg.validate()?;
        if !font_size.is_finite() || !(8.0..=96.0).contains(&font_size) {
            return Err("Visual font size is invalid".into());
        }
        Ok(Self {
            image: Arc::new(Image::from_bytes(ImageFormat::Svg, svg.source.into_bytes())),
            width: svg.width,
            height: svg.height,
            baseline: svg.baseline,
            font_size,
        })
    }
}

pub type Images = HashMap<(Kind, String), Prepared>;

mod inline;
pub(super) use inline::render;

#[cfg(test)]
mod tests;
