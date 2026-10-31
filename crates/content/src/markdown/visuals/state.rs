use super::{Images, Kind, Prepared, Renderer, Request};
use crate::markdown::{BlockKind, Doc, Mark, html, style::Theme, typography::Typography};
use gpui_kit::component::ActiveTheme;
use gpui_kit::{Context, Hsla, Task, Window};
use std::collections::{HashMap, HashSet};

/// Prepared visuals of one document, owned by the view or editor showing it.
#[derive(Default)]
pub(crate) struct Cache {
    /// Each request and the document source it renders, which for HTML is
    /// not the sanitized copy sent to the surface.
    requests: HashMap<String, (Request, String)>,
    ready: HashMap<String, Prepared>,
    tasks: HashMap<String, Task<()>>,
    /// Sanitized HTML by block source, kept while the block exists.
    sanitized: HashMap<String, String>,
}

impl Cache {
    pub fn images(&self) -> Images {
        self.ready
            .iter()
            .filter_map(|(key, image)| {
                let (request, source) = self.requests.get(key)?;
                Some(((request.kind, source.clone()), image.clone()))
            })
            .collect()
    }

    /// Request what `doc` shows and drop what it no longer does. `owner`
    /// finds this cache again when a rendering completes.
    pub fn prepare<T: 'static>(
        &mut self,
        doc: &Doc,
        foreground: Option<Hsla>,
        typography: Option<Typography>,
        window: &mut Window,
        cx: &mut Context<T>,
        owner: fn(&mut T) -> &mut Cache,
    ) {
        let Some(renderer) = cx.try_global::<Renderer>().cloned() else {
            return;
        };
        let mut theme = Theme::of(cx);
        if let Some(foreground) = foreground {
            theme.text = foreground;
        }
        let typography = typography.unwrap_or_else(|| Typography::of(cx));
        let wanted = requests(
            doc,
            &theme,
            cx.theme().is_dark(),
            typography.body.size(),
            &mut self.sanitized,
        );
        let keys: HashSet<_> = wanted.iter().map(|(request, _)| key(request)).collect();
        self.requests.retain(|key, _| keys.contains(key));
        self.ready.retain(|key, _| keys.contains(key));
        self.tasks.retain(|key, _| keys.contains(key));
        // Keyed by every HTML block present, including those that sanitize
        // to nothing, so none is sanitized again on each frame.
        let sources: HashSet<_> = doc
            .blocks
            .iter()
            .filter_map(|block| match &block.kind {
                BlockKind::Html(code) => Some(code.text.as_str()),
                _ => None,
            })
            .collect();
        self.sanitized
            .retain(|source, _| sources.contains(source.as_str()));
        for (request, source) in wanted {
            let key = key(&request);
            if self.requests.contains_key(&key) {
                continue;
            }
            self.requests.insert(key.clone(), (request.clone(), source));
            let font_size = request.font_size;
            let task = (renderer.0)(request, window, cx);
            let result_key = key.clone();
            let task = cx.spawn(async move |entity, cx| {
                let result = task
                    .await
                    .and_then(|output| Prepared::new(output, font_size));
                let _ = entity.update(cx, |state, cx| {
                    let cache = owner(state);
                    if !cache.requests.contains_key(&result_key) {
                        return;
                    }
                    if let Ok(image) = result {
                        cache.ready.insert(result_key.clone(), image);
                    }
                    cache.tasks.remove(&result_key);
                    cx.notify();
                });
            });
            self.tasks.insert(key, task);
        }
    }
}

fn key(request: &Request) -> String {
    serde_json::to_string(request).expect("Validated visual request must be serializable")
}

fn color(value: Hsla) -> String {
    let rgb = value.to_rgb();
    format!(
        "#{:02x}{:02x}{:02x}",
        (rgb.r * 255.0).round() as u8,
        (rgb.g * 255.0).round() as u8,
        (rgb.b * 255.0).round() as u8
    )
}

/// The requests a document needs, each with the source it renders.
fn requests(
    doc: &Doc,
    theme: &Theme,
    dark: bool,
    font_size: f32,
    sanitized: &mut HashMap<String, String>,
) -> Vec<(Request, String)> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |kind: Kind, source: &str, rendered: String| {
        if result.len() >= 128 || !seen.insert((kind, source.to_owned())) {
            return;
        }
        let request = Request {
            kind,
            source: rendered,
            dark,
            foreground: color(if kind == Kind::Mermaid {
                theme.surface.blend(theme.text)
            } else {
                theme.text
            }),
            background: color(theme.surface),
            accent: color(match kind {
                // Diagram edges carry information, not a control's faint hairline.
                Kind::Mermaid => theme.surface.blend(theme.text_muted),
                Kind::Html => theme.link,
                _ => theme.accent,
            }),
            font_size,
        };
        if request.validate().is_ok() {
            result.push((request, source.to_owned()));
        }
    };
    for block in &doc.blocks {
        match &block.kind {
            BlockKind::Code {
                language: Some(language),
                code,
            } if language == "mermaid" => add(Kind::Mermaid, &code.text, code.text.clone()),
            BlockKind::Math(code) => add(Kind::DisplayMath, &code.text, code.text.clone()),
            BlockKind::Html(code) => {
                let clean = sanitized
                    .entry(code.text.clone())
                    .or_insert_with(|| html::sanitize(&code.text))
                    .clone();
                // Nothing left to show; the block keeps its source instead.
                if !clean.trim().is_empty() {
                    add(Kind::Html, &code.text, clean);
                }
            }
            _ => {
                for part in block.parts() {
                    if let Some(text) = block.text_at(part) {
                        for mark in &text.marks {
                            if mark.mark == Mark::Math {
                                let source = &text.text[mark.range.clone()];
                                add(Kind::InlineMath, source, source.to_owned());
                            }
                        }
                    }
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests;
