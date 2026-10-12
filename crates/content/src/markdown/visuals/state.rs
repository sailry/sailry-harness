use super::{Images, Kind, Prepared, Renderer, Request};
use crate::markdown::{BlockKind, Doc, Mark, style::Theme, typography::Typography, view::State};
use gpui_kit::component::ActiveTheme;
use gpui_kit::{Context, Hsla, Task, Window};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(in crate::markdown) struct Cache {
    requests: HashMap<String, Request>,
    ready: HashMap<String, Prepared>,
    tasks: HashMap<String, Task<()>>,
}

impl Cache {
    pub fn images(&self) -> Images {
        self.ready
            .iter()
            .filter_map(|(key, image)| {
                let request = self.requests.get(key)?;
                Some(((request.kind, request.source.clone()), image.clone()))
            })
            .collect()
    }

    pub fn prepare(state: &mut State, window: &mut Window, cx: &mut Context<State>) {
        let Some(renderer) = cx.try_global::<Renderer>().cloned() else {
            return;
        };
        let mut theme = Theme::of(cx);
        if let Some(foreground) = state.foreground {
            theme.text = foreground;
        }
        let typography = state.typography.unwrap_or_else(|| Typography::of(cx));
        let wanted = requests(
            &state.document,
            &theme,
            cx.theme().is_dark(),
            typography.body.size(),
        );
        let keys: HashSet<_> = wanted.iter().map(key).collect();
        state.visuals.requests.retain(|key, _| keys.contains(key));
        state.visuals.ready.retain(|key, _| keys.contains(key));
        state.visuals.tasks.retain(|key, _| keys.contains(key));
        for request in wanted {
            let key = key(&request);
            if state.visuals.requests.contains_key(&key) {
                continue;
            }
            state.visuals.requests.insert(key.clone(), request.clone());
            let font_size = request.font_size;
            let task = (renderer.0)(request, window, cx);
            let result_key = key.clone();
            let task = cx.spawn(async move |entity, cx| {
                let result = task.await.and_then(|svg| Prepared::new(svg, font_size));
                let _ = entity.update(cx, |state, cx| {
                    if !state.visuals.requests.contains_key(&result_key) {
                        return;
                    }
                    if let Ok(image) = result {
                        state.visuals.ready.insert(result_key.clone(), image);
                    }
                    state.visuals.tasks.remove(&result_key);
                    cx.notify();
                });
            });
            state.visuals.tasks.insert(key, task);
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

fn requests(doc: &Doc, theme: &Theme, dark: bool, font_size: f32) -> Vec<Request> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |kind, source: &str| {
        if result.len() >= 128 || !seen.insert((kind, source.to_owned())) {
            return;
        }
        let request = Request {
            kind,
            source: source.into(),
            dark,
            foreground: color(if kind == Kind::Mermaid {
                theme.text_muted
            } else {
                theme.text
            }),
            background: color(theme.surface),
            accent: color(if kind == Kind::Mermaid {
                theme.border
            } else {
                theme.accent
            }),
            font_size,
        };
        if request.validate().is_ok() {
            result.push(request);
        }
    };
    for block in &doc.blocks {
        match &block.kind {
            BlockKind::Code {
                language: Some(language),
                code,
            } if language == "mermaid" => add(Kind::Mermaid, &code.text),
            BlockKind::Math(code) => add(Kind::DisplayMath, &code.text),
            _ => {
                for part in block.parts() {
                    if let Some(text) = block.text_at(part) {
                        for mark in &text.marks {
                            if mark.mark == Mark::Math {
                                add(Kind::InlineMath, &text.text[mark.range.clone()]);
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
